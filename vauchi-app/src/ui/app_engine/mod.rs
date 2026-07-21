// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi`, owns the active workflow engine, handles
//! navigation routing, and implements `WorkflowEngine` for all frontends.

mod app_screen;
mod ble_handshake;
mod completion;
mod completion_contact;
mod completion_forms;
// INLINE_TEST_REQUIRED: injects a completed DeviceReplacementEngine into the
// private `engine`/`screen` fields to drive the decommission-at-Complete hook.
#[cfg(test)]
mod decommission_at_complete_tests;
mod device_link;
#[cfg(all(feature = "network-http", feature = "storage"))]
mod device_link_initiator;
#[cfg(all(feature = "network-http", feature = "storage"))]
mod device_link_responder;
mod dispatch;
mod help_catalog;
// INLINE_TEST_REQUIRED: drives the `pub(super)` create_engine factory and the
// `pub(crate)` DirectTransportEngine::outgoing_card seam — crate-internal.
#[cfg(test)]
mod factory_filter_tests;
mod intercept;
mod intercept_annotations;
mod link_exchange;
mod link_responder;
mod multi_stage_exchange;
mod navigation;
mod overlays;
// INLINE_TEST_REQUIRED: injects a legacy ExchangeEngine into the private
// `engine`/`screen` fields to drive the persist-at-Complete hook.
#[cfg(test)]
mod persist_at_complete_tests;
mod result_routing;
mod routing;
mod screens;
mod screens_contacts;
mod screens_exchange;

pub use app_screen::AppScreen;
use overlays::{
    ACTION_DISMISS_DEMO_CONTACT, ACTION_GO_BACK, ACTION_OPEN_SETTINGS, ACTION_OPEN_UPDATE_LINK,
    ACTION_SYNC_NOW,
};
pub use {navigation::TabLayout, overlays::SyncChromeStatus};

use std::collections::HashMap;
use std::sync::mpsc;

use vauchi_core::api::{HandlerId, Vauchi, VauchiEvent};
use vauchi_core::exchange::capability::TransportReadiness;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::version::{APP_COMPAT_VERSION, AppUpdateStatus, VersionPolicy};

use crate::activity_log_writer::ActivityLogWriter;
use crate::notification_emitter::NotificationEmitter;
use crate::notification_types::{ActivityLogEntry, NotificationPreferences, PendingNotification};

use super::action::{ActionResult, UserAction};
use super::engine::WorkflowEngine;
use super::screen::ScreenModel;

/// Tracks which contact undo is pending (archive only — delete is now irrevocable).
#[derive(Clone, Debug)]
pub(super) enum PendingContactUndo {
    Archive,
}

/// Unified orchestrator for all frontends.
pub struct AppEngine {
    vauchi: Vauchi,
    screen: AppScreen,
    engine: Box<dyn WorkflowEngine>,
    engine_cache: HashMap<AppScreen, Box<dyn WorkflowEngine>>,
    /// Group ids chosen in the exchange group-selection preamble,
    /// carried across the ExchangeEngine->MultiStageExchange handoff so
    /// `persist_exchanged_contact` can assign the new contact + show the
    /// group on the success screen (2026-06-04-exchange-terminal-screens).
    pending_exchange_groups: Vec<String>,
    /// Navigation history stack for back-button support.
    nav_history: Vec<AppScreen>,
    /// Field pending undo after delete from MyInfoEntryDetail.
    pending_field_undo: Option<(String, vauchi_core::contact_card::ContactField)>,
    /// Contact pending undo after soft-delete or archive.
    pending_contact_undo: Option<PendingContactUndo>,
    /// Cached field type catalog (built once from SocialNetworkRegistry).
    field_catalog: vauchi_core::contact_card::FieldTypeCatalog,
    /// Transient preview-as state — contact ID being previewed (not serialized).
    pub(super) preview_as_contact: Option<String>,
    /// Device hardware capabilities reported by the frontend at startup.
    /// Used to determine exchange mode availability.
    pub(super) device_capabilities: DeviceCapabilities,
    /// Transient transport-permission ledger (T2.1b): updated from
    /// `Event::PermissionDenied`, joined with `device_capabilities` by T2.2.
    pub(super) transport_readiness: TransportReadiness,
    /// Frontend-pushed render context — Category-1 settings per ADR-047
    /// (locale + theme_id). Owned by the frontend's OS-native sandbox;
    /// pushed via `set_render_context_json` at boot and on Settings
    /// dropdown changes. S2 of
    /// `_private/docs/planning/todo/2026-05-16-settings-storage-by-sensitivity-plan.md`.
    render_context: crate::ui::RenderContext,
    /// Current app update status from the relay/CDN version policy.
    update_status: AppUpdateStatus,
    /// Whether the user has dismissed the "update available" banner.
    update_dismissed: bool,
    /// Last timestamp (seconds) when activity log was polled for OS notifications.
    last_poll_time: u64,
    /// Channel receiver for events to be persisted to the activity log.
    event_rx: mpsc::Receiver<VauchiEvent>,
    /// Active event handler ID, used to unregister on drop.
    _event_handler_id: HandlerId,
    /// IDs of the two contacts selected for merge (primary_id, secondary_id).
    /// Set when the user confirms a merge from DuplicateDetection; consumed by
    /// handle_completion for ContactMerge.
    pub(super) pending_merge: Option<(String, String)>,
    /// Whether a backup reminder toast should be shown on next main-screen render.
    pending_backup_reminder: bool,
    /// Frontend-reported network reachability (`NWPathMonitor` on iOS,
    /// `ConnectivityManager` on Android). When `false`, every emitted
    /// `ScreenModel` is decorated with an offline `Component::Banner`
    /// via `apply_offline_overlay`, so frontends never decide whether
    /// to render the banner themselves (audit
    /// `2026-04-28-lifecycle-session-residue-umbrella` item P2-D).
    /// Defaults to `true` so installs without a network monitor (CLI,
    /// tests, embedded) behave as if always-online.
    network_online: bool,
    /// Last sync attempt result. Drives the `Component::Indicator`
    /// chrome chip injected on every top-level screen by
    /// `apply_sync_chrome_overlay`. Updated in the `sync_now` handler
    /// arm after each `Vauchi::sync()` call. Defaults to
    /// `SyncChromeStatus::Idle` on engine boot.
    sync_chrome_status: SyncChromeStatus,
    /// Screen-presentation [`Command`]s accumulated from
    /// `WorkflowEngine::screen_entered` / `screen_exited` callbacks
    /// during navigation. Frontends drain via
    /// [`Self::drain_pending_commands`] after each `navigate_to` /
    /// `navigate_back` / `handle_action` to apply hardware-side state
    /// (brightness, idle timer, future orientation lock, haptics).
    /// Phase 2b of `2026-05-04-exchange-command-screen-presentation`.
    pending_commands: std::collections::VecDeque<vauchi_core::Command>,
    /// Contact id awaiting an `Event::LocationResult` after an exchange
    /// emitted `Command::LocationRequest` (ADR-051 capture-at-exchange).
    /// Recorded via `Vauchi::set_exchange_location`; a location denial /
    /// unavailability clears it.
    pending_location_contact: Option<String>,
    /// Contact id already persisted by the legacy-QR persist-at-Complete hook
    /// (`app_engine/routing.rs`). Guards against a second save on Done —
    /// re-saving the exchange ratchet would reset Double Ratchet state
    /// (`2026-06-04-exchange-terminal-screens`). Reset when a fresh exchange
    /// engine is built (`screens_exchange::rebuild_exchange_engine`).
    legacy_exchange_persisted: Option<String>,
    /// Engine-owned link-mode responder machine (slice 32l Phase 2), live
    /// only on `AppScreen::DeepLinkResponder`. See `app_engine/link_responder.rs`.
    link_responder: Option<vauchi_core::exchange::link_responder::LinkResponderSession>,
    /// Per-exchange X3DH keypair retained for the responder — signed into
    /// the v2 bootstrap, completes the exchange (ADR-050 T5b).
    link_responder_x3dh: Option<vauchi_core::exchange::X3DHKeyPair>,
    /// Engine-owned link-mode **initiator** machine (slice 32l Phase 3), live
    /// only on `AppScreen::LinkExchange`. See `app_engine/link_exchange.rs`.
    link_initiator: Option<vauchi_core::exchange::link_initiator::LinkInitiatorSession>,
    /// The initiator half of `link_responder_x3dh` (ADR-050 T5b).
    link_initiator_x3dh: Option<vauchi_core::exchange::X3DHKeyPair>,
    /// Engine-owned device-link **initiator** machine (slice 32l T3.1b), live
    /// only on `AppScreen::DeviceLinking`. See `app_engine/device_link_initiator.rs`.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    device_link_initiator: Option<device_link_initiator::DeviceLinkInitiatorHolder>,
    /// Engine-owned device-link **join** (responder) machine (M5 B3 Slice 3),
    /// live only on `AppScreen::DeviceLinkJoin`. See
    /// `app_engine/device_link_responder.rs`.
    #[cfg(all(feature = "network-http", feature = "storage"))]
    device_link_responder: Option<device_link_responder::DeviceLinkResponderHolder>,
    /// Engine-owned multi-stage exchange machine (slice 32m T1.2b), live
    /// only on `AppScreen::MultiStageExchange`. See
    /// `app_engine/multi_stage_exchange.rs`. Replaces the
    /// `vauchi-platform::MobileMultiStageSession` cycle thread; T1.2c
    /// removes the parallel cycle-thread bridge in PlatformAppEngine
    /// to avoid double-driving the active engine on mobile.
    multi_stage_session: Option<multi_stage_exchange::MultiStageHolder>,
    /// Engine-owned BLE handshake machine (slice 32m T2.2b). Built
    /// on `Event::BleConnected` (or PlatformAppEngine on BLE-eligible
    /// screen entry), torn down by `cancel_ble_handshake_session`.
    /// Replaces the `MobileBleExchangeSession` cycle thread; T2.2c
    /// routes BLE events through `forward_ble_hardware_event`.
    ble_handshake_session: Option<ble_handshake::BleHandshakeHolder>,
    /// Glance (one-sided QR) OOB state. `glance_display_nonce` is the nonce
    /// this device shows in its QR and must require as the responder;
    /// `glance_scanned` is the scanner-side binding built from a scanned QR —
    /// its presence latches this device into the scanner role.
    glance_display_nonce: Option<[u8; 16]>,
    glance_scanned: Option<crate::orchestrator::ble_handshake_machine::BleOobBinding>,
    /// The base64 OOB QR this device displays for Glance, generated ONCE on
    /// screen entry (never per-render — regenerating rotates the nonce and
    /// breaks the pin) and injected into the `BleExchangeEngine`'s screen.
    glance_display_qr: Option<String>,
}

impl AppEngine {
    /// Shorthand for the current render locale's translation lookup.
    pub(super) fn t(&self, key: &str) -> String {
        crate::i18n::get_string(self.render_context.resolved_locale(), key)
    }

    /// Returns a reference to the inner Vauchi instance.
    pub fn vauchi(&self) -> &Vauchi {
        &self.vauchi
    }

    /// Returns a mutable reference to the inner Vauchi instance.
    pub fn vauchi_mut(&mut self) -> &mut Vauchi {
        &mut self.vauchi
    }

    /// Set device hardware capabilities (reported by frontend at startup).
    ///
    /// Invalidates the exchange screen cache so mode availability is
    /// recalculated on next visit.
    pub fn set_device_capabilities(&mut self, caps: DeviceCapabilities) {
        self.device_capabilities = caps;
        self.engine_cache.remove(&AppScreen::Exchange);
    }

    /// The transport-readiness ledger (presence × permission) — consult
    /// seam for the mode picker (T2.2) + tests.
    pub fn transport_readiness(&self) -> &TransportReadiness {
        &self.transport_readiness
    }

    /// Returns the active render context (locale + theme_id) pushed
    /// by the frontend. Used by `screens.rs` to render Settings
    /// dropdown `selected` values without pulling state from the
    /// vault. See [ADR-047](../../../../_private/docs/decisions/adr-047-settings-storage-by-sensitivity.md).
    pub fn render_context(&self) -> &crate::ui::RenderContext {
        &self.render_context
    }

    /// Replace the active render context. Called from PAE
    /// `set_render_context_json` after JSON deserialization;
    /// frontends invoke this at boot and on every Settings
    /// locale/theme dropdown change. Invalidates the Settings
    /// screen cache AND — when the active screen is Settings —
    /// rebuilds the active engine so the next `current_screen()`
    /// call reflects the new dropdown `selected` value without
    /// requiring a navigate-away-and-back round trip.
    pub fn set_render_context(&mut self, ctx: crate::ui::RenderContext) {
        self.render_context = ctx;
        self.engine_cache.remove(&AppScreen::Settings);
        if matches!(self.screen, AppScreen::Settings) {
            let screen = self.screen.clone();
            self.engine = Self::create_engine(
                &self.vauchi,
                &screen,
                self.preview_as_contact.as_deref(),
                &self.device_capabilities,
                &self.transport_readiness,
                &self.render_context,
                &self.pending_exchange_groups,
                self.glance_display_qr.as_deref(),
            );
        }
    }

    /// Hot-reload design assets (themes and/or tokens) from raw JSON, then
    /// rebuild every screen. Tokens/themes affect ALL screens (not just
    /// Settings), so the whole engine cache is cleared and the active
    /// engine rebuilt — the next `current_screen()` reflects the new
    /// values. UI-shaped name; a method on the engine, no new generic
    /// (ADR-030). Errors if either JSON fails to parse (store untouched).
    pub fn reload_design_assets(
        &mut self,
        themes_json: Option<&[u8]>,
        tokens_json: Option<&[u8]>,
    ) -> Result<(), crate::theme::ThemeError> {
        if let Some(data) = themes_json {
            crate::theme::load_themes_from_bytes(data)?;
        }
        if let Some(data) = tokens_json {
            crate::theme::load_design_tokens_from_bytes(data)?;
        }
        self.engine_cache.clear();
        let screen = self.screen.clone();
        self.engine = Self::create_engine(
            &self.vauchi,
            &screen,
            self.preview_as_contact.as_deref(),
            &self.device_capabilities,
            &self.transport_readiness,
            &self.render_context,
            &self.pending_exchange_groups,
            self.glance_display_qr.as_deref(),
        );
        Ok(())
    }

    pub fn new(vauchi: Vauchi) -> Self {
        // Boot decision (audit
        // `2026-04-28-app-launch-and-identity-orchestration-in-core`
        // §2.1). Frontends call `boot()` and trust the returned
        // `ScreenModel` — they do not duplicate this decision tree.
        //
        // The "identity exists but onboarding never marked complete"
        // resume-path called out by §2.5 is gated behind a future
        // migration: existing installs ship `OnboardingProgress` at
        // default (`completed_at == None`) so honouring the flag
        // today would route every legacy user back through
        // onboarding. Until the legacy heal lands, "identity exists"
        // implies "past onboarding"; the atomic
        // `create_identity_with_onboarding` helper still closes the
        // *new* crash window for installs after this commit.
        let screen = if !vauchi.has_identity() {
            AppScreen::Onboarding
        } else if vauchi.is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            AppScreen::MyInfo
        };
        let caps = DeviceCapabilities::default();
        let initial_render_context = crate::ui::RenderContext::default();
        let engine = Self::create_engine(
            &vauchi,
            &screen,
            None,
            &caps,
            &TransportReadiness::default(),
            &initial_render_context,
            &[],
            None,
        );
        let registry = vauchi_core::social::SocialNetworkRegistry::with_defaults();
        let field_catalog = vauchi_core::contact_card::FieldTypeCatalog::new(&registry);

        let now = vauchi.clock().unix_seconds();

        // Register a permanent event handler that sends VauchiEvents to a channel
        // for deferred persistence to the activity log (ADR-031).
        // Since VauchiEvent handler must be Sync but mpsc::Sender is not Sync,
        // we wrap it in a Mutex.
        let (event_tx, event_rx) = mpsc::channel();
        let event_tx = std::sync::Mutex::new(event_tx);
        let event_handler_id = vauchi.add_event_handler(std::sync::Arc::new(move |event| {
            if let Ok(tx) = event_tx.lock() {
                // best-effort: channel send fails only if the receiver
                // was dropped (AppEngine is being torn down)
                #[allow(clippy::let_underscore_must_use)]
                let _ = tx.send(event);
            }
        }));

        // Surface safety alerts a previous session accepted but never
        // surfaced (crash between receive-commit and dispatch). At-least-once
        // from durable facts; consumers dedup by the alert nonce.
        if let Err(error) = vauchi.surface_pending_safety_alerts() {
            tracing::warn!("[AppEngine] Failed to surface pending safety alerts: {error:?}");
        }

        // Check if a backup reminder is due (only if identity exists and not on lock/onboarding).
        let pending_backup_reminder = matches!(screen, AppScreen::MyInfo | AppScreen::Contacts)
            && vauchi
                .identity()
                .and_then(|id| {
                    let fallback = id.device_info().created_at();
                    vauchi
                        .load_backup_reminder_state()
                        .ok()
                        .map(|state| state.is_reminder_due(now, fallback))
                })
                .unwrap_or(false);

        Self {
            vauchi,
            screen,
            engine,
            engine_cache: HashMap::new(),
            pending_exchange_groups: Vec::new(),
            nav_history: Vec::new(),
            pending_field_undo: None,
            pending_contact_undo: None,
            field_catalog,
            preview_as_contact: None,
            device_capabilities: DeviceCapabilities::default(),
            transport_readiness: TransportReadiness::default(),
            render_context: crate::ui::RenderContext::default(),
            update_status: AppUpdateStatus::UpToDate,
            update_dismissed: false,
            last_poll_time: now,
            event_rx,
            _event_handler_id: event_handler_id,
            pending_merge: None,
            pending_backup_reminder,
            network_online: true,
            sync_chrome_status: SyncChromeStatus::Idle,
            pending_commands: std::collections::VecDeque::new(),
            pending_location_contact: None,
            legacy_exchange_persisted: None,
            link_responder: None,
            link_responder_x3dh: None,
            link_initiator: None,
            link_initiator_x3dh: None,
            #[cfg(all(feature = "network-http", feature = "storage"))]
            device_link_initiator: None,
            #[cfg(all(feature = "network-http", feature = "storage"))]
            device_link_responder: None,
            multi_stage_session: None,
            ble_handshake_session: None,
            glance_display_nonce: None,
            glance_scanned: None,
            glance_display_qr: None,
        }
    }

    /// Drain and return all `Command`s accumulated from `WorkflowEngine`
    /// lifecycle hooks (`screen_entered` / `screen_exited`) during recent
    /// navigation. Frontends call this after every action / navigate to
    /// pick up brightness, idle-timer, and (Phase 2c) orientation-lock
    /// commands that core has emitted in response to screen transitions.
    pub fn drain_pending_commands(&mut self) -> Vec<vauchi_core::Command> {
        self.pending_commands.drain(..).collect()
    }

    /// Append commands to the pending queue. Phase 1.C.3e-v of the
    /// Hover graduation plan — `PlatformAppEngine`'s audio-listener
    /// bridge calls this to forward commands generated by the
    /// session's audio handshake (`Command::AudioEmitChallenge` /
    /// `AudioListenForResponse`) into the unified pending stream so
    /// the next `screen_envelope_to_json` drain surfaces them to the
    /// frontend.
    ///
    /// Frontends never call this directly — sessions emit via the
    /// listener path, engines emit via `ActionResult::Commands`, and
    /// the unified queue ensures both classes flow through the same
    /// drain.
    pub fn extend_pending_commands(
        &mut self,
        cmds: impl IntoIterator<Item = vauchi_core::Command>,
    ) {
        self.pending_commands.extend(cmds);
    }

    /// Check and drain a pending backup reminder.
    ///
    /// Returns `Some(ShowToast { .. })` if a backup reminder is due, `None` otherwise.
    /// Frontends should call this after initialization or after unlocking.
    /// The toast action id is `"backup_now"` — pressing it navigates to Backup.
    pub fn drain_backup_reminder(&mut self) -> Option<ActionResult> {
        if self.pending_backup_reminder {
            self.pending_backup_reminder = false;
            // Record that we showed a reminder
            if let Ok(mut state) = self.vauchi.load_backup_reminder_state() {
                state.record_reminder_shown();
                // best-effort: reminder-shown bookkeeping; failure here
                // means the user may see the reminder again sooner
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.vauchi.save_backup_reminder_state(&state);
            }
            Some(ActionResult::ShowToast {
                message: "You haven't backed up in a while. Back up now to protect your identity."
                    .into(),
                undo_action_id: Some("backup_now".into()),
                undo_label: Some(self.t("backup.wizard.create")),
            })
        } else {
            None
        }
    }

    /// Enter preview-as mode: show MyInfo as seen by the given contact.
    ///
    /// Sets transient state, invalidates the MyInfo cache, and navigates to MyInfo
    /// in PreviewAs view mode. The state is cleared by handling "exit-preview".
    pub fn preview_as(&mut self, contact_id: String) -> ScreenModel {
        self.preview_as_contact = Some(contact_id);
        self.invalidate_screen(&AppScreen::MyInfo);
        self.navigate_to(AppScreen::MyInfo)
    }

    /// Drain pending OS notifications.
    ///
    /// Processes buffered events through [`ActivityLogWriter`] and
    /// [`NotificationEmitter`], returning notifications for the frontend
    /// to display. Each call clears the buffer, so notifications are
    /// never returned twice.
    ///
    /// Frontends should call this after receiving an event callback.
    pub fn drain_pending_notifications(&mut self) -> Vec<PendingNotification> {
        let new_entries = self.drain_events_to_log();
        if new_entries.is_empty() {
            return Vec::new();
        }

        let prefs = NotificationPreferences::default();
        let locale = self.render_context.resolved_locale();
        NotificationEmitter::evaluate(&new_entries, &prefs, locale, |contact_id| {
            self.vauchi
                .get_contact(contact_id)
                .ok()
                .flatten()
                .map(|c| c.display_name().to_string())
                .unwrap_or_else(|| format!("Contact {}", &contact_id[..8.min(contact_id.len())]))
        })
    }

    pub fn current_app_screen(&self) -> &AppScreen {
        &self.screen
    }

    pub fn has_identity(&self) -> bool {
        self.vauchi.has_identity()
    }

    /// Notify core that the app was backgrounded (or is about to resume).
    ///
    /// If a password is set and the app is not already locked or in onboarding,
    /// navigates to the lock screen and returns the lock `ScreenModel`.
    /// Returns `None` if no action is needed (no password, already locked,
    /// or onboarding).
    ///
    /// Frontends should call this on app lifecycle events:
    /// - iOS: `scenePhase == .background`
    /// - Android: `onPause()` or `Lifecycle.Event.ON_STOP`
    /// - Desktop: window focus lost (optional, configurable)
    pub fn handle_app_backgrounded(&mut self) -> Option<ScreenModel> {
        // Don't lock during onboarding (no identity yet) or if already locked
        if matches!(self.screen, AppScreen::Lock | AppScreen::Onboarding) {
            return None;
        }
        // Only lock if a password is set
        if !self.vauchi.is_password_enabled().unwrap_or(false) {
            return None;
        }
        Some(self.navigate_to(AppScreen::Lock))
    }

    /// Update the app's version status from a relay/CDN version policy.
    ///
    /// Evaluates the policy against the current `APP_COMPAT_VERSION` and
    /// resets the dismissed flag if an update becomes required.
    pub fn set_version_policy(&mut self, policy: &VersionPolicy) {
        self.update_status =
            policy.evaluate(APP_COMPAT_VERSION, self.vauchi.clock().unix_seconds());
        if matches!(self.update_status, AppUpdateStatus::UpdateRequired { .. }) {
            self.update_dismissed = false;
        }
    }
}

impl WorkflowEngine for AppEngine {
    fn current_screen(&self) -> ScreenModel {
        let screen = self.engine.current_screen();
        let screen = self.apply_update_overlay(screen);
        let screen = self.apply_offline_overlay(screen);
        let screen = self.apply_sync_chrome_overlay(screen);
        let screen = self.apply_nav_chrome_overlay(screen);
        let screen = self.apply_demo_contact_overlay(screen);
        self.apply_screen_id_metadata(self.apply_accessibility_overlay(screen))
    }

    #[tracing::instrument(level = "debug", skip_all, name = "app.handle_action")]
    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        let action = dispatch::normalize_inline_confirm_action(action);
        self.drain_events_to_log();
        // Global-chrome + top-level navigation guards (sync, backup
        // reminder, update link, tab nav, system back, settings gear,
        // demo-contact dismiss). See `dispatch::intercept_global_chrome`.
        if let Some(result) = self.intercept_global_chrome(&action) {
            return result;
        }

        // OS/share-sheet/messaging link opened.
        // See `dispatch::intercept_link_opened`.
        if let Some(result) = self.intercept_link_opened(&action) {
            return result;
        }

        // Mode-picker grant affordance (`grant:<mode>:<requirement>`): re-learn
        // a denied OS permission and re-render the picker. See
        // `screens_exchange::intercept_grant_permission`.
        if let Some(result) = self.intercept_grant_permission(&action) {
            return result;
        }

        self.persist_settings_toggle(&action);
        self.persist_consent_toggle(&action);

        if let Some(result) = self.intercept_exit_preview(&action) {
            return result;
        }

        if let Some(result) = self.intercept_edit_avatar(&action) {
            return result;
        }

        if let Some(result) = self.intercept_add_field(&action) {
            return result;
        }

        if let Some(result) = self.intercept_settings_action(&action) {
            return result;
        }

        if let Some(result) = self.intercept_decoy_contacts_action(&action) {
            return result;
        }

        if let Some(result) = self.intercept_info_requested(&action) {
            return result;
        }

        // Detail-screen interception (MyInfo entry detail, contact detail).
        // See `dispatch::intercept_contact_screen`.
        if let Some(result) = self.intercept_contact_screen(&action) {
            return result;
        }

        if let Some(result) = self.intercept_tag_delete(&action) {
            return result;
        }

        if let Some(result) = self.intercept_place_delete(&action) {
            return result;
        }

        if let Some(result) = self.intercept_tag_promotion(&action) {
            return result;
        }

        if let Some(result) = self.intercept_contact_facets(&action) {
            return result;
        }

        // List-screen CTAs, duplicate merge/dismiss, recovery actions, and
        // unarchive. See `dispatch::intercept_list_and_recovery`.
        if let Some(result) = self.intercept_list_and_recovery(&action) {
            return result;
        }

        if let Some(result) = self.handle_undo(&action) {
            return result;
        }

        let result = self.engine.handle_action(action);
        let result = self.route_result(result);
        // Slice 32l T3.1b: feed typed DeviceLink* ActionResults into the engine-owned machine.
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.dispatch_device_link_side_effects(&result);
        let result = self.resolve_validation_error(result);
        self.apply_update_overlay_to_result(result)
    }
}

impl AppEngine {
    /// Advance every live relay session (device-link initiator/responder,
    /// link-mode initiator/responder, multi-stage exchange) one protocol step
    /// and tick the active engine's wall-clock. No-op for each idle session.
    /// Shared by `poll_notifications` (the OS-notification cadence) and
    /// `on_wakeup` (the core-scheduled wakeup, ADR-044 Am2a Option C) —
    /// retiring the frontend's `requires_poll` loop that called
    /// `poll_notifications` directly. Events logged by the advances surface as
    /// notifications on the next `poll_notifications` / `on_wakeup`.
    pub(super) fn advance_relay_sessions(&mut self) {
        self.drain_events_to_log();

        // Slice 32l T3.1b: advance the device-link initiator one relay step (no-op when idle).
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.advance_device_link_session();
        // M5 B3 Slice 3: advance the device-link join (responder) one relay step.
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.advance_device_link_responder_session();
        // ADR-049: advance the link-mode responder one relay step
        // (no-op off the DeepLinkResponder screen / with no live machine).
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.advance_link_responder_session();
        // ADR-049: advance the link-mode initiator one relay step
        // (no-op off the LinkExchange screen / with no live machine).
        #[cfg(all(feature = "network-http", feature = "storage"))]
        self.advance_link_initiator_session();
        // Slice 32m T1.2b: advance the multi-stage machine one
        // protocol step (no-op when idle / no active session).
        self.advance_multi_stage_session();

        let now = self.vauchi.clock().unix_seconds();
        // Active-engine wall-clock tick — no-op unless bounded-wait (cable
        // DirectTransport `Waiting` fails a peerless stall; ADR-021, T1.3).
        self.engine.tick(now);
    }

    /// The shell's platform wakeup fired — a desktop in-process interval, an
    /// iOS `BGAppRefreshTask`, or an Android `WorkManager` task, scheduled
    /// from a prior [`vauchi_core::Command::ScheduleWakeup`]. Run all work now
    /// due (the same relay/exchange advance + activity-log poll as
    /// [`Self::poll_notifications`]) and emit the *next* `ScheduleWakeup` so
    /// the shell re-arms. Core owns *when* the heartbeat is due (ADR-044 Am2a
    /// Option C); the shell owns only the native wakeup mechanism.
    ///
    /// Returns the OS notifications to post; the next schedule rides
    /// `pending_commands` — the caller drains it via
    /// [`Self::drain_pending_commands`] after this returns. Elapsed-based and
    /// idempotent: a delayed, coalesced, or skipped wake is safe (the
    /// underlying advances/poll are no-ops when nothing is due). Public so
    /// `PlatformAppEngine` can expose it via UniFFI. The frontend bootstraps
    /// the loop by calling this once at launch.
    pub fn on_wakeup(&mut self) -> Vec<PendingNotification> {
        let notifications = self.poll_notifications();
        self.pending_commands.push_back(self.compute_next_wakeup());
        notifications
    }

    /// Compute the next app-heartbeat wakeup window (ADR-044 Am2a Option C).
    /// Core is the single authority on *when*; the shell only executes the
    /// resulting `ScheduleWakeup`. First cut: a fixed ~30 s foreground cadence
    /// (matching the historical notification poll). The elapsed-based due-math
    /// (wall `Clock` for calendar-relative token rotation, `MonotonicClock`
    /// for runtime-elapsed) refines the *values* without changing the command
    /// shape or the shell contract.
    fn compute_next_wakeup(&self) -> vauchi_core::Command {
        vauchi_core::Command::ScheduleWakeup {
            earliest_secs: 30,
            deadline_secs: 90,
            min_interval_secs: 30,
        }
    }

    /// Poll the activity log and produce pending OS notifications.
    /// Public so PlatformAppEngine can expose it via UniFFI.
    pub fn poll_notifications(&mut self) -> Vec<PendingNotification> {
        self.advance_relay_sessions();

        let now = self.vauchi.clock().unix_seconds();

        // Fetch raw rows from the activity log since the last poll.
        let rows = match self.vauchi.activity_log_poll(self.last_poll_time, now) {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("poll_notifications: activity_log_poll failed: {e}");
                return Vec::new();
            }
        };

        if rows.is_empty() {
            return Vec::new();
        }

        // Advance watermark based on rows *fetched*, not rows that survive
        // parsing/filtering. Unparsable or filtered rows must not cause
        // unbounded re-processing on every subsequent poll.
        self.last_poll_time = now;

        let entries: Vec<_> = rows
            .into_iter()
            .filter_map(|row| {
                let entry = serde_json::from_str::<ActivityLogEntry>(&row.payload).ok()?;
                Some((row.event_key, entry))
            })
            .collect();

        if entries.is_empty() {
            return Vec::new();
        }

        let prefs = NotificationPreferences {
            contact_added_enabled: self.vauchi.config().contact_added_notifications,
            // Default-on card-update heartbeat (M4 S3), now honoring the
            // persisted Settings toggle (S3a2). Per-contact mute is a Tier-1
            // follow-up.
            card_update_enabled: self.vauchi.config().card_update_notifications,
        };
        let locale = self.render_context.resolved_locale();

        // Resolve contact names for body text
        let name_resolver = |contact_id: &str| {
            self.vauchi
                .storage()
                .contacts()
                .load_contact(contact_id)
                .ok()
                .flatten()
                .map(|c| c.display_name().to_string())
                .unwrap_or_else(|| "Unknown contact".to_string())
        };

        NotificationEmitter::evaluate(&entries, &prefs, locale, name_resolver)
    }

    /// Convert `ValidationError` into `UpdateScreen` with the error injected
    /// into the matching component. This ensures frontends never receive
    /// `ValidationError` and never need to patch the `ScreenModel` themselves.
    fn resolve_validation_error(&self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::ValidationError {
                component_id,
                message,
            } => {
                let screen = self
                    .engine
                    .current_screen()
                    .with_validation_error(&component_id, message);
                ActionResult::UpdateScreen(screen)
            }
            other => other,
        }
    }

    /// Decorate a `NavigateTo`/`UpdateScreen` result's `ScreenModel` for
    /// the wire (overlays + `apply_screen_id_metadata`), mirroring
    /// `current_screen()` so results and re-reads agree. Runs after
    /// `route_result`, so internal sub-state `screen_id` routing is
    /// unaffected.
    fn apply_update_overlay_to_result(&self, result: ActionResult) -> ActionResult {
        match result {
            ActionResult::UpdateScreen(screen) => {
                ActionResult::UpdateScreen(self.apply_screen_id_metadata(
                    self.apply_offline_overlay(self.apply_update_overlay(screen)),
                ))
            }
            ActionResult::NavigateTo(screen) => {
                ActionResult::NavigateTo(self.apply_screen_id_metadata(
                    self.apply_offline_overlay(self.apply_update_overlay(screen)),
                ))
            }
            other => other,
        }
    }

    /// Drain the event receiver and write all events to the activity log.
    ///
    /// Returns newly inserted `(event_key, ActivityLogEntry)` pairs.
    /// Called before operations that read from the activity log (notifications)
    /// or when data mutations may have occurred (user actions).
    fn drain_events_to_log(&mut self) -> Vec<(String, ActivityLogEntry)> {
        let mut events = Vec::new();
        while let Ok(event) = self.event_rx.try_recv() {
            events.push(event);
        }

        if events.is_empty() {
            return Vec::new();
        }

        let now = self.vauchi.clock().unix_seconds();

        match ActivityLogWriter::write(self.vauchi.storage(), &events, now) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!("drain_events_to_log: ActivityLogWriter::write failed: {e}");
                Vec::new()
            }
        }
    }
}
