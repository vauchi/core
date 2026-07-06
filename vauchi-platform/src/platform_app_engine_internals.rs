// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Non-exported internals of [`PlatformAppEngine`] — sidecar file
//! persistence, shred plumbing, onboarding self-heal, and the
//! feature-gated content-updates wrappers. No `#[uniffi::export]`
//! item lives here; the binding surface stays in
//! `platform_app_engine.rs`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
    /// Build a `SecureStorage` bridge from the keychain set via
    /// `set_platform_keychain`. Errs if none is set (B7 shred path).
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
                detail: "Platform keychain not set. Call set_platform_keychain() first.".into(),
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
        let purge_t = vauchi.build_relay_transport(self.relay_url.clone(), 10_000);
        let rev_t = vauchi.build_relay_transport(self.relay_url.clone(), 10_000);
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

    /// Feature-gated content-update check (B7 batch 2). Returns
    /// `MobileUpdateStatus::Disabled` when the `content-updates` Cargo
    /// feature is off — matches legacy `VauchiPlatform::check_content_updates`.
    pub(crate) fn check_content_updates_dispatch(&self) -> crate::content::MobileUpdateStatus {
        #[cfg(feature = "content-updates")]
        {
            self.check_content_updates_impl_engine()
        }
        #[cfg(not(feature = "content-updates"))]
        {
            crate::content::MobileUpdateStatus::Disabled
        }
    }

    /// Whole content-update cycle: check, then apply only when updates
    /// are available. Owns the sequencing frontends used to duplicate
    /// (ADR-021/ADR-043 — F-3, pure-functional-core program record).
    pub(crate) fn run_content_update_cycle_dispatch(
        &self,
    ) -> crate::content::MobileContentCycleOutcome {
        let status = self.check_content_updates_dispatch();
        let apply = matches!(status, crate::content::MobileUpdateStatus::UpdatesAvailable)
            .then(|| self.apply_content_updates_dispatch());
        crate::content::content_cycle_outcome(&status, apply.as_ref())
    }

    /// Feature-gated content-update apply (B7 batch 2). Mirrors the
    /// legacy `VauchiPlatform::apply_content_updates` semantics —
    /// returns `Disabled` when the feature is off.
    pub(crate) fn apply_content_updates_dispatch(&self) -> crate::content::MobileApplyResult {
        #[cfg(feature = "content-updates")]
        {
            self.apply_content_updates_impl_engine()
        }
        #[cfg(not(feature = "content-updates"))]
        {
            crate::content::MobileApplyResult::Disabled
        }
    }

    /// Detect transitions in/out of session-bound screens
    /// (`MultiStageExchange`, `DeviceLinking`) and manage the
    /// corresponding session lifecycle. Called after every operation
    /// that mutates the active screen.
    pub(crate) fn after_screen_transition(&self, pre: AppScreen) -> Result<(), MobileError> {
        let post = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        // T1.2c: the AppEngine-owned machine handles its own
        // lifecycle via `sync_multi_stage_lifecycle` (called from
        // `navigate_to_internal`). The cycle-thread bridge is dead;
        // this method becomes a no-op for multi-stage. The
        // platform-side `ensure_multi_stage_session` /
        // `cancel_multi_stage_session` remain on `self` for the test
        // helpers (T3.1 deletes them).
        let _ = (pre, post);
        Ok(())
    }

    /// Internal accessor: `engine` Mutex. Used by the Pair 5
    /// device-link wiring in `platform_app_engine_device_link.rs`.
    pub(crate) fn engine(&self) -> &Arc<Mutex<AppEngine>> {
        &self.engine
    }
}

// ── Content updates internals (B7 batch 2 — feature-gated) ─────────
//
// These mirror the `VauchiPlatform::*_content_updates_impl` methods
// in `mobile_content.rs` line-for-line. Once D3 deletes the legacy
// `VauchiPlatform` surface, these become the only copies.

#[cfg(feature = "content-updates")]
impl PlatformAppEngine {
    fn check_content_updates_impl_engine(&self) -> crate::content::MobileUpdateStatus {
        use vauchi_app::content::{ContentConfig, ContentManager};

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config, vauchi_core::clock::SystemClock::shared()) {
            Ok(m) => m,
            Err(_) => {
                return crate::content::MobileUpdateStatus::CheckFailed;
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => {
                return crate::content::MobileUpdateStatus::CheckFailed;
            }
        };

        rt.block_on(async { manager.check_for_updates().await.into() })
    }

    fn apply_content_updates_impl_engine(&self) -> crate::content::MobileApplyResult {
        use vauchi_app::content::{ContentConfig, ContentManager};

        let config = ContentConfig {
            storage_path: self
                .storage_path
                .parent()
                .unwrap_or(&self.storage_path)
                .to_path_buf(),
            remote_updates_enabled: true,
            ..Default::default()
        };

        let manager = match ContentManager::new(config, vauchi_core::clock::SystemClock::shared()) {
            Ok(m) => m,
            Err(_) => {
                return crate::content::MobileApplyResult::Error;
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(_) => {
                return crate::content::MobileApplyResult::Error;
            }
        };

        rt.block_on(async {
            match manager.apply_updates().await {
                Ok(result) => result.into(),
                Err(_) => crate::content::MobileApplyResult::Error,
            }
        })
    }
}

impl PlatformAppEngine {
    /// Fire `on_screens_invalidated` on the direct listener, if any.
    /// Used by paths whose state change produces no `ActionResult` for
    /// the frontend to render (machine-held protocol advances).
    pub(crate) fn fire_screens_invalidated(&self, screen_ids: Vec<String>) {
        let listener = self
            .direct_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(screen_ids);
        }
    }
}
