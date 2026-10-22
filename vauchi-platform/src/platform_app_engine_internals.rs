// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-exported internals of [`PlatformAppEngine`] — sidecar file
//! persistence, shred plumbing, onboarding self-heal, and the
//! feature-gated content-updates wrappers. No `#[uniffi::export]`
//! item lives here; the binding surface stays in
//! `platform_app_engine.rs`.

use std::path::PathBuf;
use std::sync::atomic::Ordering;

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;

use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

/// Self-heal: if the engine is parked on `Onboarding` but identity now
/// exists in storage (a sibling `Vauchi` instance — `VauchiPlatform`
/// on iOS/Android — wrote it after this AppEngine was constructed),
/// jump to the post-auth `default_screen()`. Called from every UniFFI
/// entry that returns a rendered screen, so the very next read after
/// identity creation reflects the post-auth UI without the frontend
/// hand-coding the navigation. Workflow decision lives in core
/// (ADR-021 Humble UI). Idempotent — once `screen != Onboarding`,
/// this is a no-op.
pub(crate) fn self_heal_post_auth(engine: &mut AppEngine) {
    if matches!(engine.current_app_screen(), AppScreen::Onboarding) && engine.has_identity() {
        let target = engine.default_screen();
        // `navigate_to` (not `_internal`) pushes Onboarding onto the
        // nav history. That's harmless — the user can't reasonably
        // navigate "back" to Onboarding once an identity exists, and
        // any back-navigation falls through to MyInfo via
        // `navigate_back`'s default. The companion fix in
        // `AppEngine::navigate_to_internal` calls
        // `vauchi.refresh_identity_from_storage()` so the new screen's
        // engine sees the on-disk identity.
        engine.navigate_to(target);
    }
}

impl PlatformAppEngine {
    /// One scheduler tick, returning the `on_wakeup` envelope with the
    /// scheduler's next run (#603).
    pub(crate) fn scheduler_tick(&self) -> Result<String, MobileError> {
        let mut engine_slot = self.lock_engine()?;
        let engine = open_engine(&mut engine_slot)?;
        let (_outcome, next_run) =
            engine
                .scheduler_sync_tick()
                .map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
        crate::json_helpers::wakeup_envelope_to_json(&[], &[next_run])
    }

    /// Build a `SecureStorage` bridge from the keychain the engine was
    /// opened with. Errs if it has none (B7 shred path).
    pub(crate) fn shred_keychain_bridge(&self) -> Result<crate::KeychainBridge, MobileError> {
        let lock = self
            .platform_keychain
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        let callback = lock
            .as_ref()
            .ok_or_else(|| MobileError::Other {
                detail: "Platform keychain not set. Open the engine with open_with_keychain()."
                    .into(),
            })?
            .clone();
        Ok(crate::KeychainBridge { callback })
    }

    /// Data directory (parent of the storage db) for shred operations.
    pub(crate) fn shred_data_dir(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .to_path_buf()
    }

    /// Build the (purge, revocation) relay senders for hard/panic shred
    /// from the live engine `Vauchi` + the configured relay URL (B7 1b).
    /// Both are best-effort — send failures don't abort the shred.
    pub(crate) fn shred_senders(
        &self,
        vauchi: &Vauchi,
        sender_id: &str,
    ) -> (crate::MobileRelaySender, crate::MobileRelaySender) {
        let purge_t = vauchi.build_relay_transport(&self.relay_url, 10_000);
        let rev_t = vauchi.build_relay_transport(&self.relay_url, 10_000);
        (
            crate::MobileRelaySender::from_transport(purge_t, self.relay_url.clone(), sender_id),
            crate::MobileRelaySender::from_transport(rev_t, self.relay_url.clone(), sender_id),
        )
    }
}

impl PlatformAppEngine {
    /// File path holding the in-progress recovery proof, parallel to
    /// the SQLite database. Mirrors the legacy `VauchiPlatform` layout
    /// so both surfaces observe the same on-disk state during the
    /// Phase-C migration window.
    pub(crate) fn recovery_proof_path(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".recovery_proof")
    }

    /// File path holding the aha-moments tracker JSON (B7 batch 5).
    /// Mirrors `VauchiPlatform::aha_moments_path` so both surfaces
    /// observe the same on-disk state during the Phase-C window.
    fn aha_moments_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".aha_moments")
    }

    pub(crate) fn load_aha_tracker_engine(&self) -> vauchi_core::AhaMomentTracker {
        let path = self.aha_moments_path_engine();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::AhaMomentTracker::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::AhaMomentTracker::new()
        }
    }

    pub(crate) fn save_aha_tracker_engine(
        &self,
        tracker: &vauchi_core::AhaMomentTracker,
    ) -> Result<(), MobileError> {
        let path = self.aha_moments_path_engine();
        let data = tracker.to_json().map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        Ok(())
    }

    /// File path holding the demo-contact tracker JSON (B7 batch 5).
    fn demo_contact_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".demo_contact")
    }

    pub(crate) fn load_demo_state_engine(&self) -> vauchi_core::DemoContactState {
        let path = self.demo_contact_path_engine();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::DemoContactState::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::DemoContactState::default()
        }
    }

    pub(crate) fn save_demo_state_engine(
        &self,
        state: &vauchi_core::DemoContactState,
    ) -> Result<(), MobileError> {
        let path = self.demo_contact_path_engine();
        let data = state.to_json().map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        std::fs::write(&path, data).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        Ok(())
    }

    /// File path holding the persisted sync-flags JSON (B7 batch 18).
    /// Mirrors the aha-moments / demo-contact sidecar layout.
    /// File path holding the pinned TLS certificate PEM (B7 batch 21).
    /// Existence of the file = pinning enabled. Empty / missing = disabled.
    pub(crate) fn cert_pin_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".cert_pin")
    }

    /// Starts Core's own content cycle once a shell listens, so a
    /// background update can re-render (vauchi/private#733 D10). The first
    /// cycle waits for the app to settle.
    #[cfg(feature = "content-updates")]
    pub(crate) fn start_content_scheduler(&self) {
        const SETTLE: std::time::Duration = std::time::Duration::from_secs(30);
        let Ok(mut slot) = self.content_scheduler.lock() else {
            return;
        };
        if slot.is_some() {
            return;
        }
        let engine = self.self_ref.clone();
        *slot = vauchi_app::content::ContentScheduler::spawn(
            SETTLE,
            vauchi_app::content::ContentConfig::default().check_interval,
            move || {
                if let Some(engine) = engine.upgrade() {
                    engine.run_scheduled_content_cycle();
                }
            },
        )
        .ok();
    }

    /// Asks Core's content scheduler for a cycle now, when it runs.
    pub(crate) fn request_content_cycle(&self) {
        #[cfg(feature = "content-updates")]
        if let Ok(scheduler) = self.content_scheduler.lock()
            && let Some(scheduler) = scheduler.as_ref()
        {
            scheduler.request_cycle();
        }
    }

    /// One scheduled cycle: within the person's setting, and on applied
    /// content the screens rebuild and the shell is told to re-render.
    /// `None` when the setting kept it from running.
    pub(crate) fn run_scheduled_content_cycle(
        &self,
    ) -> Option<crate::content::MobileContentCycleOutcome> {
        let (wanted, source) = self.lock_engine().ok().and_then(|mut slot| {
            open_engine(&mut slot).ok().and_then(|engine| {
                engine.content_updates_allowed().then(|| {
                    (
                        engine.render_context().wanted_locale_codes(),
                        content_source_for(engine),
                    )
                })
            })
        })?;
        let outcome = self.run_content_update_cycle_dispatch(wanted, source);
        if outcome.applied {
            if let Ok(mut slot) = self.lock_engine()
                && let Ok(engine) = open_engine(&mut slot)
            {
                crate::pae_dispatch::engagement::invalidate_content_screens(engine);
            }
            if let Ok(notifier) = self.event_notifier.lock()
                && let Some(notifier) = notifier.as_ref()
            {
                notifier.notify(Vec::<String>::new());
            }
        }
        Some(outcome)
    }

    /// Whole content-update cycle: check, then apply only when updates
    /// are available, fetching only `wanted_locales` (vauchi/private#733
    /// D7). Owns the sequencing frontends used to duplicate (ADR-021/ADR-043
    /// — F-3, pure-functional-core program record). Disabled when the
    /// `content-updates` feature is off.
    pub(crate) fn run_content_update_cycle_dispatch(
        &self,
        wanted_locales: Vec<String>,
        source: ContentSourceArg,
    ) -> crate::content::MobileContentCycleOutcome {
        #[cfg(feature = "content-updates")]
        {
            self.run_content_update_cycle_engine(wanted_locales, source)
        }
        #[cfg(not(feature = "content-updates"))]
        {
            let _ = (wanted_locales, source);
            crate::content::content_cycle_outcome(
                &crate::content::MobileUpdateStatus::Disabled,
                None,
            )
        }
    }

    /// Fire `on_presentation_invalidated` on the direct listener, if any.
    /// Used by paths whose state changes outside a dispatch, so no command
    /// batch reaches the frontend on its own (machine-held protocol
    /// advances).
    pub(crate) fn fire_presentation_invalidated(&self) {
        let notifier = self
            .event_notifier
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(notifier) = notifier {
            notifier.notify(std::iter::empty::<String>());
        }
    }
}

// ── Content updates internals (B7 batch 2 — feature-gated) ─────────

/// Where a content cycle fetches from: the relay over OHTTP
/// (vauchi/private#733 D8).
#[cfg(feature = "content-updates")]
pub(crate) type ContentSourceArg = Option<vauchi_app::content::ContentSourceHandle>;
#[cfg(not(feature = "content-updates"))]
pub(crate) type ContentSourceArg = ();

#[cfg(feature = "content-updates")]
pub(crate) fn content_source_for(engine: &AppEngine) -> ContentSourceArg {
    Some(vauchi_app::content::relay_content_source(engine.vauchi()))
}

#[cfg(not(feature = "content-updates"))]
pub(crate) fn content_source_for(_engine: &AppEngine) -> ContentSourceArg {}

#[cfg(feature = "content-updates")]
impl PlatformAppEngine {
    fn run_content_update_cycle_engine(
        &self,
        wanted_locales: Vec<String>,
        source: ContentSourceArg,
    ) -> crate::content::MobileContentCycleOutcome {
        use crate::content::{MobileApplyResult, MobileUpdateStatus, content_cycle_outcome};
        use vauchi_app::content::{ContentConfig, ContentManager};

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            wanted_locales: Some(wanted_locales),
            source,
            ..Default::default()
        };
        let Ok(manager) = ContentManager::new(config, vauchi_core::clock::SystemClock::shared())
        else {
            return content_cycle_outcome(&MobileUpdateStatus::CheckFailed, None);
        };
        let (status, apply) = manager.run_update_cycle_blocking();
        let status: MobileUpdateStatus = status.into();
        let apply = apply.map(|result| result.map_or(MobileApplyResult::Error, Into::into));
        content_cycle_outcome(&status, apply.as_ref())
    }
}

/// The open engine, or "storage locked" while Core's storage-lock screens
/// hold storage closed (vauchi/private#580).
pub(crate) fn open_engine(slot: &mut Option<AppEngine>) -> Result<&mut AppEngine, MobileError> {
    slot.as_mut().ok_or_else(|| MobileError::StorageError {
        detail: "Storage is locked".into(),
    })
}

impl PlatformAppEngine {
    /// The engine slot. An engine whose storage a shred deleted is
    /// replaced by one opened on a fresh install first (vauchi/private#599);
    /// the slot is released while reopening, so the locked-start lock is
    /// never taken while the engine lock is held. A caller arriving
    /// mid-reopen waits for it instead of finding the slot empty
    /// (vauchi/private#293).
    pub(crate) fn lock_engine(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Option<AppEngine>>, MobileError> {
        let lock_failed = |e: String| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        };
        let mut slot = self.engine.lock().map_err(|e| lock_failed(e.to_string()))?;
        while slot.is_none() && self.reopening.load(Ordering::SeqCst) {
            slot = self
                .reopened
                .wait(slot)
                .map_err(|e| lock_failed(e.to_string()))?;
        }
        if !slot.as_mut().is_some_and(AppEngine::take_storage_shredded) {
            return Ok(slot);
        }
        let Some(shredded) = slot.take() else {
            return Ok(slot);
        };
        self.reopening.store(true, Ordering::SeqCst);
        drop(slot);
        let reopened = self.reopen_after_shred(shredded);
        let slot = self.engine.lock().map_err(|e| lock_failed(e.to_string()));
        self.reopening.store(false, Ordering::SeqCst);
        self.reopened.notify_all();
        reopened?;
        slot
    }

    pub(crate) fn lock_locked_start(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, Option<crate::platform_app_engine_locked::LockedStart>>,
        MobileError,
    > {
        self.locked.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })
    }
}
