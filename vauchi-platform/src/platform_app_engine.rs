// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unified AppEngine wrapper for mobile/desktop platforms via UniFFI.
//!
//! `PlatformAppEngine` wraps `AppEngine` and exposes the same
//! JSON-based transport format used by `Mobile*Workflow` objects. Unlike those
//! individual workflow wrappers, `PlatformAppEngine` manages navigation, engine
//! lifecycle, and data persistence as a single unified entry point.
//!
//! Mobile apps create this alongside `VauchiPlatform`:
//! - `VauchiPlatform` handles lifecycle, storage, and hardware-bound operations
//! - `PlatformAppEngine` handles navigation, screen rendering, and user actions
//!
//! After mutations via `VauchiPlatform`, call `invalidate_all()` so the next
//! `current_screen_json()` rebuilds engines with fresh data from storage.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::types::{
    MobileLocale, MobileNotificationCategory, MobilePendingNotification, MobileTabInfo,
    MobileTabLayout,
};
use vauchi_app::notification_types::NotificationCategory as CoreNotificationCategory;
use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::{HandlerId, Vauchi, VauchiConfig, VauchiEvent};
use vauchi_core::crypto::SymmetricKey;

use crate::error::MobileError;
use crate::json_helpers::{
    action_result_envelope_to_json, app_screen_from_json, hardware_event_envelope_to_json,
    screen_envelope_to_json, screen_to_json, user_action_from_json,
};

// ── PlatformEventListener ──────────────────────────────────────────

/// Callback interface for async state-change notifications from core.
///
/// Frontends implement this trait (in Swift/Kotlin via UniFFI) and register
/// it with [`PlatformAppEngine::set_event_listener`]. Core calls
/// `on_screens_invalidated` when background operations (sync, delivery,
/// device link) change data that affects rendered screens.
///
/// On receiving the callback, frontends should call `invalidate_screen_json`
/// or `invalidate_all` and re-render the affected screens.
#[uniffi::export(callback_interface)]
pub trait PlatformEventListener: Send + Sync {
    /// Called when one or more screens have stale data due to a background
    /// operation. `screen_ids` contains the `screen_id` values of affected
    /// screens (e.g., `["contacts", "delivery_status"]`).
    fn on_screens_invalidated(&self, screen_ids: Vec<String>);
}

/// Shared slot for the registered `PlatformEventListener`. Used both
/// by `set_event_listener` (to mirror the listener arc) and by the
/// Pair 4 multi-stage bridge listener (cycle thread side) to fire
/// invalidation callbacks for the multi-stage screen.
pub(crate) type DirectListenerSlot = Arc<Mutex<Option<Arc<Box<dyn PlatformEventListener>>>>>;

// ── PlatformAppEngine ───────────────────────────────────────────────

/// Unified navigation and screen engine for mobile/desktop platforms.
///
/// Wraps `AppEngine` with JSON-based FFI transport.
/// Manages screen navigation, engine lifecycle, and form dialog persistence.
///
/// # Usage from Swift/Kotlin
///
/// ```swift
/// let engine = try PlatformAppEngine(
///     dataDir: dataDir,
///     relayUrl: "https://relay.vauchi.app",
///     storageKeyBytes: keyBytes
/// )
///
/// // Get current screen
/// let screenJson = try engine.currentScreenJson()
///
/// // Handle user action
/// let resultJson = try engine.handleActionJson(
///     actionJson: "{\"ActionPressed\": {\"action_id\": \"get_started\"}}"
/// )
///
/// // Navigate to a screen
/// let screenJson = try engine.navigateToJson(screenJson: "\"Exchange\"")
///
/// // After VauchiPlatform mutations, invalidate
/// try engine.invalidateAll()
/// ```
#[derive(uniffi::Object)]
pub struct PlatformAppEngine {
    /// Wrapped in `Arc<Mutex<…>>` rather than plain `Mutex<…>` so the
    /// Pair 4 multi-stage bridge listener (running on the session's
    /// cycle thread) can hold a clone and mutate the active engine
    /// without requiring `Arc<Self>` plumbing through every entry point.
    engine: Arc<Mutex<AppEngine>>,
    /// Active event listener handler ID, used to unregister on replacement.
    event_handler_id: Mutex<Option<HandlerId>>,
    /// Direct handle to the active `PlatformEventListener`. The
    /// existing `set_event_listener` path routes via `VauchiEvent` →
    /// `affected_screens(...)`; the multi-stage cycle thread does not
    /// emit `VauchiEvent`s, so this slot lets the bridge call
    /// `on_screens_invalidated` directly when the engine state changes
    /// from a listener callback (Pair 4 of pure-humble-ui-retire-native-screens).
    direct_listener: DirectListenerSlot,
    /// Storage path retained for in-place session creation. Mirrors
    /// `VauchiPlatform::storage_path` so content-update internals can
    /// resolve the data directory without a sibling `VauchiPlatform`.
    storage_path: PathBuf,
    /// Platform keychain for crypto-shred `DomainCommand`s (B7). Set
    /// post-construction via `set_platform_keychain`, mirroring
    /// `VauchiPlatform`'s slot. `None` until the frontend wires it.
    platform_keychain: Mutex<Option<Arc<dyn crate::MobilePlatformKeychain>>>,
    /// Relay URL retained for building shred purge/revocation senders
    /// (B7 Phase 1b) — PAE can't reopen a fresh relay `Vauchi` (no stored
    /// storage key), so hard/panic shred builds senders off the live engine
    /// `Vauchi` + this URL.
    relay_url: String,
}

/// Self-heal: if the engine is parked on `Onboarding` but identity now
/// exists in storage (a sibling `Vauchi` instance — `VauchiPlatform`
/// on iOS/Android — wrote it after this AppEngine was constructed),
/// jump to the post-auth `default_screen()`. Called from every UniFFI
/// entry that returns a rendered screen, so the very next read after
/// identity creation reflects the post-auth UI without the frontend
/// hand-coding the navigation. Workflow decision lives in core
/// (ADR-021 Humble UI). Idempotent — once `screen != Onboarding`,
/// this is a no-op.
fn self_heal_post_auth(engine: &mut AppEngine) {
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
    fn shred_keychain_bridge(&self) -> Result<crate::KeychainBridge, MobileError> {
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
    fn shred_data_dir(&self) -> PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .to_path_buf()
    }

    /// Build the (purge, revocation) relay senders for hard/panic shred
    /// from the live engine `Vauchi` + the configured relay URL (B7 1b).
    /// Both are best-effort — send failures don't abort the shred.
    fn shred_senders(
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

#[uniffi::export]
impl PlatformAppEngine {
    /// Create a new PlatformAppEngine with platform-provided secure key.
    ///
    /// This creates its own `Vauchi` instance backed by the same database
    /// as `VauchiPlatform`. After mutations via `VauchiPlatform`, call
    /// `invalidate_all()` to refresh cached engines.
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        relay_url: String,
        storage_key_bytes: Vec<u8>,
    ) -> Result<Arc<Self>, MobileError> {
        let data_path = PathBuf::from(&data_dir);

        std::fs::create_dir_all(&data_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] =
            storage_key_bytes
                .try_into()
                .map_err(|_| MobileError::StorageError {
                    detail: "Storage key must be exactly 32 bytes".to_string(),
                })?;
        let storage_key =
            SymmetricKey::try_from_bytes(key_array).map_err(|_| MobileError::StorageError {
                detail: "Degenerate storage key rejected".to_string(),
            })?;

        let config = VauchiConfig::with_storage_path(&storage_path)
            .with_relay_url(&relay_url)
            .with_storage_key(storage_key.clone());

        let vauchi = Vauchi::new(config).map_err(|e| MobileError::Other {
            detail: e.to_string(),
        })?;

        Ok(Arc::new(Self {
            engine: Arc::new(Mutex::new(AppEngine::new(vauchi))),
            event_handler_id: Mutex::new(None),
            direct_listener: Arc::new(Mutex::new(None)),
            storage_path,
            platform_keychain: Mutex::new(None),
            relay_url,
        }))
    }

    /// Returns the cold-start `ScreenModel` JSON for whatever the
    /// app's current persistent state is.
    ///
    /// Frontends call this **once** on cold start (after constructing
    /// `PlatformAppEngine`) and render the result. They do **not**
    /// branch on `has_identity` / `is_password_enabled` /
    /// `is_onboarding_complete` themselves — that decision lives
    /// inside core's `AppEngine::new()` boot logic and the
    /// idempotent `self_heal_post_auth` self-heal that follows.
    ///
    /// Equivalent to `current_screen_json()` plus an explicit
    /// contract: the audit
    /// `2026-04-28-app-launch-and-identity-orchestration-in-core`
    /// §2.1 elevates "first read after instance construction" from
    /// "implicit / by convention" to "named API method", so iOS
    /// `AppState` and Android `UiState` shadow enums can be deleted
    /// without ambiguity. Subsequent reads use `current_screen_json`.
    pub fn boot(&self) -> Result<String, MobileError> {
        self.current_screen_json()
    }

    /// Returns the current screen as a JSON string.
    ///
    /// The JSON structure matches `ScreenModel` from vauchi-core.
    pub fn current_screen_json(&self) -> Result<String, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        // Self-heal: when AppEngine was constructed without an identity
        // (engine = OnboardingEngine, screen = Onboarding) and a sibling
        // Vauchi instance — `VauchiPlatform` on iOS/Android — has since
        // written identity to the shared DB, the live engine is stale.
        // Auto-navigate to the post-auth default so frontends never have
        // to hand-code "after onboarding, navigate to MyInfo": the
        // workflow decision lives in core (ADR-021 Humble UI). Idempotent
        // — once `screen != Onboarding`, this is a no-op.
        self_heal_post_auth(&mut engine);
        screen_to_json(&engine.current_screen())
    }

    /// Returns the mobile bottom-tab bar metadata (id, label, icon,
    /// badge count) with labels resolved from the supplied `locale`.
    ///
    /// Frontends render the returned `MobileTabInfo` directly — no
    /// local screen-to-tab map or label lookup needed (G1 of the
    /// frontend pure-renderer remediation; ADR-021 / ADR-038).
    pub fn tab_info(&self, locale: MobileLocale) -> Result<Vec<MobileTabInfo>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        self_heal_post_auth(&mut engine);
        Ok(engine
            .tab_info(locale.into())
            .into_iter()
            .map(MobileTabInfo::from)
            .collect())
    }

    /// Returns desktop-sidebar metadata — all top-level navigable
    /// screens with locale-resolved labels. Wider than `tab_info()`
    /// because desktop frames accommodate more entries than a phone
    /// bottom-tab bar. Use this from macOS / Windows / linux-gtk /
    /// linux-qt sidebars so they stop maintaining their own screen →
    /// label match tables (§6 pure-renderer remediation).
    pub fn sidebar_items(&self, locale: MobileLocale) -> Result<Vec<MobileTabInfo>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine
            .sidebar_items(locale.into())
            .into_iter()
            .map(MobileTabInfo::from)
            .collect())
    }

    /// Returns the navigation chrome for `layout` — the mobile bottom-tab
    /// bar (`Mobile`) or the desktop sidebar (`Desktop`) — with labels
    /// resolved from `locale`.
    ///
    /// Merges the former `tab_info` / `sidebar_items` wrappers: the
    /// frontend passes its layout (the value it already gives
    /// `current_tab_id`) instead of picking a form-factor-named method,
    /// so the form-factor decision stays in core (ADR-023 Amendment 1).
    /// The engine peers `tab_info()` / `sidebar_items()` remain — cabi
    /// serves the C-ABI desktop frontends through them.
    pub fn nav_items(
        &self,
        layout: MobileTabLayout,
        locale: MobileLocale,
    ) -> Result<Vec<MobileTabInfo>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        self_heal_post_auth(&mut engine);
        let items = match layout {
            MobileTabLayout::Mobile => engine.tab_info(locale.into()),
            MobileTabLayout::Desktop => engine.sidebar_items(locale.into()),
        };
        Ok(items.into_iter().map(MobileTabInfo::from).collect())
    }

    /// Handles a user action (as JSON) and returns the result as JSON.
    ///
    /// The action JSON must match the `UserAction` enum format.
    /// The result JSON matches the `ActionResult` enum. Note:
    /// `ValidationError` is never returned — validation errors are
    /// resolved into `UpdateScreen` with the error injected into the
    /// matching component's `validation_error` field.
    pub fn handle_action_json(&self, action_json: String) -> Result<String, MobileError> {
        let action = user_action_from_json(&action_json)?;
        // Pair 4 — pre-action retry detection: when the user presses
        // Retry on the multi-stage screen the underlying session must
        // restart, not just the engine view-state. Cancel + recreate
        // before the engine handles the action so the post-retry state
        // pushes (Idle → Advertising → …) come from a fresh cycle thread.
        let pre_screen = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        let on_multi_stage = matches!(pre_screen, AppScreen::MultiStageExchange { .. });
        let is_retry = matches!(
            &action,
            vauchi_app::ui::UserAction::ActionPressed { action_id }
                if action_id == vauchi_app::ui::MULTI_STAGE_RETRY_ACTION_ID
        );
        if on_multi_stage && is_retry {
            // T1.2c: rebuild the AppEngine-owned machine in place so
            // the next advance emits a fresh INIT QR. The cycle-thread
            // path is dead — `cancel_multi_stage_session` /
            // `ensure_multi_stage_session` on `self` are kept around
            // for the test helpers (T3.1 deletes them).
            let mode = match &pre_screen {
                AppScreen::MultiStageExchange { mode } => *mode,
                _ => {
                    return Err(MobileError::Other {
                        detail: "retry dispatched off multi-stage screen".into(),
                    });
                }
            };
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.cancel_multi_stage_session();
            engine.ensure_multi_stage_session(mode);
        }

        // Pair 4 — auto-route the peer-scan QR text into the live
        // cycle-thread session. The QR-scanner Component (iOS
        // QrCodeView, Android QrCodeComponent) emits
        // `UserAction::TextChanged { component_id: "peer_scan", value }`
        // per the existing `exchange_qr.rs` single-direction contract.
        // On the multi-stage screen the engine has no session handle,
        // so without this side-effect the scan would be dropped on
        // `MultiStageExchangeEngine::handle_action`'s default
        // `UpdateScreen` fall-through. Mirrors the `QrScanned`
        // hardware-event auto-route in `handle_hardware_event`.
        if on_multi_stage
            && let vauchi_app::ui::UserAction::TextChanged {
                component_id,
                value,
            } = &action
            && component_id == vauchi_app::ui::MULTI_STAGE_PEER_SCAN_COMPONENT_ID
        {
            // T1.2c: route the scan through the AppEngine-owned
            // machine. The QR-scanner Component emits TextChanged
            // with the scanned data; we wrap it in a synthetic
            // `Event::QrScanned` so the same machine ingress
            // handles both this UserAction path and the
            // `handle_hardware_event` path below.
            let qr_event = vauchi_core::Event::QrScanned {
                data: value.clone(),
            };
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            let m_event = engine.forward_multi_stage_hardware_event(&qr_event);
            engine.apply_multi_stage_event(m_event);
        }

        let (result, pending_commands) = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            let result = engine.handle_action(action);
            // Phase 2b: drain the screen-presentation lifecycle commands
            // that any navigation triggered by this action accumulated.
            // Frontends process them via the same dispatch path as
            // `ActionResult::Commands` — they're surfaced separately in
            // the envelope so the action result and the lifecycle hooks
            // stay independently typed.
            let cmds = engine.drain_pending_commands();
            (result, cmds)
        };
        self.after_screen_transition(pre_screen)?;
        action_result_envelope_to_json(&result, &pending_commands)
    }

    /// Handle a hardware event from the frontend during an exchange (ADR-031).
    ///
    /// Frontends call this when hardware reports results (QR scanned, BLE
    /// data received, etc.). Returns the serialized `ActionResult` JSON if
    /// the event produced a result, or `None` if the current screen doesn't
    /// handle hardware events.
    ///
    /// # Usage from Swift
    ///
    /// ```swift
    /// if let resultJson = try engine.handleHardwareEvent(
    ///     event: .qrScanned(data: scannedData)
    /// ) {
    ///     let result = try decoder.decode(ActionResult.self, from: resultJson.data(using: .utf8)!)
    ///     applyResult(result)
    /// }
    /// ```
    pub fn handle_hardware_event(&self, event: crate::MobileEvent) -> Result<String, MobileError> {
        let hw_event: vauchi_core::Event = event.into();
        // Pair 4 — auto-route QrScanned to the live multi-stage session
        // when the multi-stage screen is active. The frontend never has
        // to know there is a session: it just emits the `QrScanned`
        // hardware event and core delivers it to the protocol.
        let on_multi_stage = matches!(
            self.engine
                .lock()
                .map_err(|e| MobileError::Other {
                    detail: format!("Lock failed: {e}"),
                })?
                .current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        );

        // Resolve the optional `ActionResult` from whichever routing path the
        // event takes. The `Command`s it emits accumulate in `pending_commands`
        // regardless of path, and are drained into the envelope at the end — the
        // fix for command-driven transports (BLE / NFC data / audio responses),
        // whose commands were previously stranded and never executed.
        let action_result: Option<vauchi_app::ui::ActionResult> =
            if on_multi_stage && let vauchi_core::Event::QrScanned { .. } = &hw_event {
                // T1.2c: route through the AppEngine-owned machine.
                let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                    detail: format!("Lock failed: {e}"),
                })?;
                let m_event = engine.forward_multi_stage_hardware_event(&hw_event);
                engine.apply_multi_stage_event(m_event);
                None
            } else {
                // BLE/Magic completion P2 — a peer discovery on the BLE
                // exchange screen builds the AppEngine-owned handshake session.
                // The role is decided from the peer's advertised tiebreak
                // token (in `adv_data`), matching `BleExchangeFlow`'s connect
                // decision. Idempotent; falls through to the engine below,
                // which emits `BleConnect` for the tiebreak winner. Once the
                // session is active, the `BleConnected`/data events route into
                // the real machine via the gate that follows.
                if let vauchi_core::Event::BleDeviceDiscovered { adv_data, .. } = &hw_event {
                    let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                        detail: format!("Lock failed: {e}"),
                    })?;
                    if matches!(engine.current_app_screen(), AppScreen::BleExchange { .. }) {
                        engine.start_ble_handshake_on_discovery(adv_data);
                    }
                }

                // Slice 32m T2.2c — BLE event routing into the AppEngine-owned
                // `BleHandshakeMachine`, gated on an active session. Additive on top
                // of the regular `engine.handle_hardware_event` below so the existing
                // `ExchangeEngine::BleExchangeFlow` proximity path runs undisturbed.
                if matches!(
                    &hw_event,
                    vauchi_core::Event::BleConnected { .. }
                        | vauchi_core::Event::BleCharacteristicNotified { .. }
                        | vauchi_core::Event::BleCharacteristicRead { .. }
                        | vauchi_core::Event::BleMtuNegotiated { .. }
                        | vauchi_core::Event::BleDisconnected { .. }
                ) {
                    let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                        detail: format!("Lock failed: {e}"),
                    })?;
                    if engine.ble_handshake_session_active() {
                        let m_event = engine.forward_ble_hardware_event(&hw_event);
                        // P3 — on Completed, persist the decrypted peer card +
                        // Double Ratchet as an exchanged contact. Inert for
                        // every other machine event.
                        engine.apply_ble_machine_event(m_event);
                    }
                }

                // ADR-031: biometric unlock arrives as a hardware event. Core
                // consults its duress-PIN state and pads the wall-clock to
                // `BIOMETRIC_UNLOCK_MIN_DURATION` so the unlock-screen timing can't
                // leak whether duress is configured.
                if let vauchi_core::Event::BiometricUnlockSucceeded = &hw_event {
                    let outcome = self
                        .engine
                        .lock()
                        .map_err(|e| MobileError::Other {
                            detail: format!("Lock failed: {e}"),
                        })?
                        .vauchi_mut()
                        .biometric_unlock_check()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                    Some(vauchi_app::ui::ActionResult::BiometricUnlockOutcome { outcome })
                } else {
                    let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                        detail: format!("Lock failed: {e}"),
                    })?;
                    engine.handle_hardware_event(hw_event)
                }
            };

        // Drain every command the event produced and ship it alongside the
        // result so the frontend executes it on the hardware.
        let commands = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .drain_pending_commands();
        hardware_event_envelope_to_json(action_result.as_ref(), &commands)
    }

    /// Advance the animated QR to the next frame.
    ///
    /// Returns the updated ScreenModel JSON when the active engine has animated
    /// frames to cycle (currently only `ExchangeEngine` on the ShowQr step), or
    /// `None` otherwise. Frontends call this on a ~100ms timer while displaying
    /// the "Share Your Code" screen to cycle V6-sized QR chunks for reliable
    /// 240p camera decode.
    ///
    /// # Usage from Swift
    ///
    /// ```swift
    /// if let frameJson = try engine.advanceQrFrameJson() {
    ///     applyScreen(decode(frameJson))
    /// }
    /// ```
    pub fn advance_qr_frame_json(&self) -> Result<Option<String>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        match engine.advance_qr_frame() {
            Some(screen) => Ok(Some(screen_to_json(&screen)?)),
            None => Ok(None),
        }
    }

    /// Navigate back in the history stack.
    ///
    /// Returns the previous screen model as JSON envelope:
    /// `{"screen": <ScreenModel>, "commands": [<Command>, ...]}`.
    /// `commands` carries any screen-presentation `Command`s emitted by
    /// the lifecycle hooks of the outgoing + incoming engines (Phase 2b).
    pub fn navigate_back_json(&self) -> Result<String, MobileError> {
        let pre_screen = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        let (model, pending_commands) = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            let model = engine.navigate_back();
            let cmds = engine.drain_pending_commands();
            (model, cmds)
        };
        self.after_screen_transition(pre_screen)?;
        screen_envelope_to_json(&model, &pending_commands)
    }

    /// Whether a back step exists in core's nav-history stack.
    ///
    /// Frontends drive their back affordance / `BackHandler` from this
    /// instead of inferring "is this a core-driven screen?" from a
    /// frontend-side screen-id map (ADR-043: no constructed nav targets).
    /// Tier-0 of the CoreScreenIdMap rework.
    pub fn can_go_back(&self) -> Result<bool, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine.can_go_back())
    }

    /// Handle an incoming `vauchi://exchange?...` deep link URI.
    ///
    /// On a successful parse, navigates to the consent gate (the new
    /// `AppScreen::DeepLinkConsent`) and returns the rendered screen
    /// model as JSON. Frontends render the result and forward
    /// `UserAction::ActionPressed { action_id: "grant" | "deny" }` via
    /// the existing `handle_action_json` path.
    ///
    /// On parse failure, returns a typed `MobileError::InvalidInput`
    /// with `field` set to one of:
    /// - `"deep_link_scheme"` — URI scheme is not `vauchi`
    /// - `"deep_link_host"` — URI host is not `exchange`
    /// - `"deep_link_format"` — URI uses the legacy path-component
    ///   form, or query parameters are missing/malformed
    ///
    /// Per ADR-021 (Humble UI): the consent decision is policy and
    /// lives in core; frontends only forward the raw URI string and
    /// render the returned screen. Phase 1 of
    /// `2026-04-25-deeplink-consent-orchestrator`.
    pub fn handle_deep_link_uri(&self, uri: String) -> Result<String, MobileError> {
        use vauchi_core::exchange::link_mode::{DeepLinkParseError, parse_exchange_deep_link};

        let payload = parse_exchange_deep_link(&uri).map_err(|err| match err {
            DeepLinkParseError::InvalidScheme => MobileError::InvalidInput {
                field: "deep_link_scheme".into(),
                detail: "URI scheme must be vauchi://".into(),
            },
            DeepLinkParseError::InvalidHost => MobileError::InvalidInput {
                field: "deep_link_host".into(),
                detail: "URI host must be exchange".into(),
            },
            DeepLinkParseError::LegacyPathForm => MobileError::InvalidInput {
                field: "deep_link_format".into(),
                detail: "deep link uses an old path-based format; ask the sender for a fresh link"
                    .into(),
            },
            DeepLinkParseError::MalformedQuery => MobileError::InvalidInput {
                field: "deep_link_format".into(),
                detail: "deep link query parameters are missing or malformed".into(),
            },
        })?;

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let model = engine.navigate_to(vauchi_app::ui::AppScreen::DeepLinkConsent { payload });
        screen_to_json(&model)
    }

    /// Returns the canonical screen-id of the parent tab the active
    /// screen belongs to under the given layout, or `None` for
    /// transient overlays (Lock, FormDialog).
    ///
    /// `Mobile` matches the 5-tab bottom nav from `tab_info`;
    /// `Desktop` matches the 14-tab sidebar from `sidebar_items`.
    /// Frontends use this to keep tab/sidebar selection in sync with
    /// the active screen without maintaining their own
    /// `screen_id` → `parent_tab` map (§1D pure-renderer remediation).
    pub fn current_tab_id(&self, layout: MobileTabLayout) -> Result<Option<String>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine.current_tab_id(layout.into()).map(|s| s.to_string()))
    }

    /// Invalidate all cached engines.
    ///
    /// Call this after mutations via `VauchiPlatform` so the next
    /// `current_screen_json()` rebuilds engines with fresh data.
    pub fn invalidate_all(&self) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine.invalidate_all();
        Ok(())
    }

    /// Invalidate a specific screen's cached engine.
    ///
    /// The screen JSON must match the `AppScreen` enum format.
    pub fn invalidate_screen_json(&self, screen_json: String) -> Result<(), MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine.invalidate_screen(&screen);
        Ok(())
    }

    /// Returns whether the user has created an identity.
    ///
    /// Used by frontends to decide between onboarding and main UI.
    pub fn has_identity(&self) -> Result<bool, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine.has_identity())
    }

    /// Run one periodic sync tick.
    ///
    /// Frontends call this from their platform-scheduler handler
    /// (`BGTaskScheduler` on iOS, `WorkManager` on Android). Per-tick
    /// behaviour lives in core: gate on identity / OHTTP key,
    /// honour the throttle window, delegate to `Vauchi::sync()`.
    /// Frontends do not duplicate the "sync if due" logic.
    ///
    /// Returns the [`vauchi_core::VauchiSyncOutcome`] serialised as
    /// JSON so the platform shell can log/observe without binding
    /// the full sync types over UniFFI.
    ///
    /// Audit `2026-04-28-lifecycle-session-residue-umbrella` P2-C.
    pub fn periodic_sync_tick(&self) -> Result<String, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let outcome = engine
            .vauchi_mut()
            .periodic_sync_tick()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        serde_json::to_string(&format!("{outcome:?}")).map_err(|e| MobileError::Other {
            detail: format!("serialize sync outcome: {e}"),
        })
    }

    /// Push render-context preferences (locale + theme_id) from
    /// frontend OS-native storage to core.
    ///
    /// Category-1 settings per
    /// [ADR-047](../../../../_private/docs/decisions/adr-047-settings-storage-by-sensitivity.md):
    /// frontends own the canonical copy in OS-native sandboxed
    /// storage (`SharedPreferences`, `UserDefaults`, …) and push
    /// the active values to core at app boot + on every Settings
    /// dropdown change. Core uses them to render Settings dropdown
    /// `selected` values (S3 of the implementation plan) and,
    /// later, to resolve locale-keyed strings into ScreenModel.
    ///
    /// JSON shape: `{ "locale": "de", "theme_id": "cyber" }`.
    /// Both fields optional — `null` / absent means "frontend has
    /// no value yet". Field names are UI-shaped to preserve the
    /// humble-allowlist invariant (no domain words).
    pub fn set_render_context_json(&self, json: String) -> Result<(), MobileError> {
        let ctx: vauchi_app::ui::RenderContext =
            serde_json::from_str(&json).map_err(|e| MobileError::Other {
                detail: format!("Invalid render context JSON: {e}"),
            })?;
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine.set_render_context(ctx);
        Ok(())
    }

    /// Report frontend-observed network reachability to core.
    ///
    /// Frontends call this from their platform reachability monitor
    /// (`NWPathMonitor` on iOS, `ConnectivityManager` on Android)
    /// callback. While `online == false`, every emitted
    /// `ScreenModel` carries a presentational offline `Component::Banner`
    /// that frontends render automatically — no
    /// `MainViewModel.isOnline` mirror flag, no `OfflineBanner()`
    /// switch in the view tree.
    ///
    /// Audit `2026-04-28-lifecycle-session-residue-umbrella` P2-D.
    pub fn set_network_online(&self, online: bool) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine.set_network_online(online);
        Ok(())
    }

    /// Set the platform keychain for crypto-shred `DomainCommand`s (B7).
    /// Mirrors `VauchiPlatform::set_platform_keychain`; frontends call it
    /// post-construction like the other PAE setters. Used only by the
    /// shred dispatch arms.
    pub fn set_platform_keychain(&self, keychain: Box<dyn crate::MobilePlatformKeychain>) {
        if let Ok(mut lock) = self.platform_keychain.lock() {
            *lock = Some(Arc::from(keychain));
        }
    }

    /// Notify core that the app was backgrounded.
    ///
    /// If a password is set and the app is not already locked,
    /// returns the lock screen JSON. Otherwise returns null.
    /// Frontends should call on `scenePhase == .background` (iOS)
    /// or `onPause()` (Android).
    pub fn handle_app_backgrounded(&self) -> Result<Option<String>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        match engine.handle_app_backgrounded() {
            Some(screen) => {
                let json = serde_json::to_string(&screen).map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                Ok(Some(json))
            }
            None => Ok(None),
        }
    }

    /// Poll core for pending OS notifications to render.
    pub fn poll_notifications(&self) -> Result<Vec<MobilePendingNotification>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let items = engine.poll_notifications();
        // T1.2c: the multi-stage machine just advanced inside
        // `engine.poll_notifications`. The cycle-thread bridge that
        // used to fire `on_screens_invalidated` on every state change
        // is dead, so fire one ourselves whenever a machine is held —
        // the frontend re-fetches `current_screen_json` and reflects
        // the new QR / state. Cheap over-fire (frontend renders are
        // idempotent against the same screen JSON), correct in every
        // case the cycle thread used to cover.
        let multi_stage_active = engine.multi_stage_session_active()
            && matches!(
                engine.current_app_screen(),
                AppScreen::MultiStageExchange { .. }
            );
        drop(engine);
        if multi_stage_active {
            let listener = self
                .direct_listener
                .lock()
                .ok()
                .and_then(|guard| guard.clone());
            if let Some(listener) = listener {
                listener.on_screens_invalidated(vec!["multi_stage_exchange".into()]);
            }
        }
        let mapped = items
            .into_iter()
            .map(|n| MobilePendingNotification {
                event_key: n.event_key,
                category: match n.category {
                    CoreNotificationCategory::EmergencyAlert => {
                        MobileNotificationCategory::EmergencyAlert
                    }
                    CoreNotificationCategory::ContactAdded => {
                        MobileNotificationCategory::ContactAdded
                    }
                },
                title: n.title,
                body: n.body,
                contact_id: n.contact_id,
            })
            .collect();
        Ok(mapped)
    }

    /// Report device hardware capabilities.
    ///
    /// Call once at startup after querying platform hardware APIs.
    /// Determines which exchange modes are available.
    pub fn set_device_capabilities_json(
        &self,
        capabilities_json: String,
    ) -> Result<(), MobileError> {
        let caps: vauchi_core::exchange::capability::types::DeviceCapabilities =
            serde_json::from_str(&capabilities_json).map_err(|e| MobileError::Other {
                detail: format!("Invalid capabilities JSON: {e}"),
            })?;
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine.set_device_capabilities(caps);
        Ok(())
    }

    /// Register a listener for async state-change notifications.
    ///
    /// Core calls `on_screens_invalidated` when background operations
    /// (sync, delivery receipts, device link) change data that affects
    /// rendered screens. Replaces any previously registered listener.
    ///
    /// # Threading — IMPORTANT
    ///
    /// The callback may fire **on the same thread** that called
    /// `handle_action_json` (synchronous event dispatch). The callback
    /// **must not** call back into `PlatformAppEngine` methods directly —
    /// doing so would deadlock on the internal Mutex. Always dispatch
    /// to a separate queue/thread before touching the engine.
    ///
    /// # Usage from Swift
    ///
    /// ```swift
    /// class MyListener: PlatformEventListener {
    ///     func onScreensInvalidated(screenIds: [String]) {
    ///         DispatchQueue.main.async {  // REQUIRED — never call engine synchronously
    ///             for id in screenIds {
    ///                 try? engine.invalidateScreenJson(screenJson: "\"\(id)\"")
    ///             }
    ///             self.reloadCurrentScreen()
    ///         }
    ///     }
    /// }
    /// try engine.setEventListener(listener: MyListener())
    /// ```
    ///
    /// # Usage from Kotlin
    ///
    /// ```kotlin
    /// class MyListener : PlatformEventListener {
    ///     override fun onScreensInvalidated(screenIds: List<String>) {
    ///         viewModelScope.launch {  // REQUIRED — never call engine synchronously
    ///             for (id in screenIds) {
    ///                 engine.invalidateScreenJson("\"$id\"")
    ///             }
    ///             reloadCurrentScreen()
    ///         }
    ///     }
    /// }
    /// engine.setEventListener(MyListener())
    /// ```
    pub fn set_event_listener(
        &self,
        listener: Box<dyn PlatformEventListener>,
    ) -> Result<(), MobileError> {
        let listener = Arc::new(listener);

        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;

        let mut handler_id_slot = self
            .event_handler_id
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        if let Some(old_id) = handler_id_slot.take() {
            engine.vauchi().remove_event_handler(old_id);
        }

        // Register a new handler that maps VauchiEvents to screen IDs
        let listener_clone = Arc::clone(&listener);
        let new_id = engine
            .vauchi()
            .add_event_handler(Arc::new(move |event: VauchiEvent| {
                let screen_ids = vauchi_app::ui::affected_screens(&event);
                if !screen_ids.is_empty() {
                    let owned: Vec<String> = screen_ids.into_iter().map(String::from).collect();
                    listener_clone.on_screens_invalidated(owned);
                }
            }));

        *handler_id_slot = Some(new_id);

        // Mirror the listener arc into the direct-call slot used by
        // bridges (Pair 4 multi-stage exchange) that bypass the
        // VauchiEvent path. Held under a separate lock so callers
        // observing one do not block the other.
        let mut direct = self
            .direct_listener
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        *direct = Some(listener);
        Ok(())
    }

    // ── Recovery (Phase B2 — collapse-vauchi-platform-into-app-engine) ──
    //
    // Wraps the recovery domain that previously only lived on
    // `VauchiPlatform`. Frontends migrating in Phase C1 / C7 stop
    // touching the legacy struct and route every recovery operation
    // through the engine. Cache invalidation targets the `Recovery`
    // and `RecoveryHelp` screens so reads after a write reflect the
    // mutation without an explicit `invalidate_*` call from the caller.

    // ── DomainCommand dispatch (Phase B7 — collapse-vauchi-platform-into-app-engine) ──
    //
    // Long-tail domain operations that don't justify their own typed
    // method. The R3 hybrid keeps Recovery / Emergency Broadcast /
    // Device Linking as direct typed methods (B2/B3/B4); everything
    // else collapses into `DomainCommand`. New domains are added
    // batch-by-batch in their own MRs.
    //
    // First batch (this MR): Consent (5 variants).

    /// Dispatch a typed domain command. Pattern match on the
    /// returned [`DomainCommandResult`] in the calling code; see
    /// `core/vauchi-platform/src/domain_command.rs` for the
    /// variant set.
    pub fn dispatch_domain_command(
        &self,
        command: crate::domain_command::DomainCommand,
    ) -> Result<crate::domain_command::DomainCommandResult, MobileError> {
        use crate::domain_command::{DomainCommand, DomainCommandResult};

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;

        match command {
            DomainCommand::GrantConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                manager
                    .grant(vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RevokeConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                manager
                    .revoke(vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::CheckConsent { consent_type } => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let value = manager
                    .check(&vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::GetConsentStatus { consent_type } => {
                let status = engine
                    .vauchi()
                    .get_consent_status(vauchi_core::api::ConsentType::from(consent_type))
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::ConsentStatus {
                    status: crate::types::MobileConsentStatus::from(status),
                })
            }
            DomainCommand::GetConsentRecords => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::ConsentManager::new(storage);
                let records =
                    manager
                        .export_consent_log_with_version()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::ConsentRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileConsentRecord::from)
                        .collect(),
                })
            }

            // ── Content Updates (B7 batch 2) ──
            DomainCommand::IsContentUpdatesSupported => Ok(DomainCommandResult::Bool {
                value: cfg!(feature = "content-updates"),
            }),
            DomainCommand::CheckContentUpdates => {
                let status = self.check_content_updates_dispatch();
                Ok(DomainCommandResult::UpdateStatus { status })
            }
            DomainCommand::ApplyContentUpdates => {
                let result = self.apply_content_updates_dispatch();
                // Content updates can refresh on-disk content cache;
                // invalidate any screen that reads social-network labels.
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::ApplyResult { result })
            }
            DomainCommand::ReloadSocialNetworks => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                let networks = registry
                    .all()
                    .iter()
                    .map(|sn| crate::types::MobileSocialNetwork {
                        id: sn.id().to_string(),
                        display_name: sn.display_name().to_string(),
                        url_template: sn.profile_url_template().to_string(),
                    })
                    .collect();
                Ok(DomainCommandResult::SocialNetworks { networks })
            }

            // ── Aha Moments (B7 batch 5) ──
            DomainCommand::HasSeenAhaMoment { moment_type } => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Bool {
                    value: tracker.has_seen(moment_type.into()),
                })
            }
            DomainCommand::TryTriggerAhaMoment { moment_type } => {
                let mut tracker = self.load_aha_tracker_engine();
                let core_type: vauchi_core::AhaMomentType = moment_type.into();
                let moment = if let Some(m) = tracker.try_trigger(core_type) {
                    self.save_aha_tracker_engine(&tracker)?;
                    Some(crate::types::MobileAhaMoment {
                        moment_type,
                        title: m.title().to_string(),
                        message: m.message(),
                        has_animation: m.has_animation(),
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::AhaMomentOpt { moment })
            }
            DomainCommand::TryTriggerAhaMomentWithContext {
                moment_type,
                context,
            } => {
                let mut tracker = self.load_aha_tracker_engine();
                let core_type: vauchi_core::AhaMomentType = moment_type.into();
                let moment = if let Some(m) = tracker.try_trigger_with_context(core_type, context) {
                    self.save_aha_tracker_engine(&tracker)?;
                    Some(crate::types::MobileAhaMoment {
                        moment_type,
                        title: m.title().to_string(),
                        message: m.message(),
                        has_animation: m.has_animation(),
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::AhaMomentOpt { moment })
            }
            DomainCommand::AhaMomentsSeenCount => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Count {
                    value: tracker.seen_count() as u32,
                })
            }
            DomainCommand::AhaMomentsTotalCount => {
                let tracker = self.load_aha_tracker_engine();
                Ok(DomainCommandResult::Count {
                    value: tracker.total_count() as u32,
                })
            }
            DomainCommand::ResetAhaMoments => {
                let mut tracker = self.load_aha_tracker_engine();
                tracker.reset();
                self.save_aha_tracker_engine(&tracker)?;
                Ok(DomainCommandResult::Unit)
            }

            // ── Demo Contact (B7 batch 5) ──
            DomainCommand::InitDemoContactIfNeeded => {
                let storage = engine.vauchi().storage();
                let contacts = storage
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                if !contacts.is_empty() {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                let mut state = self.load_demo_state_engine();
                if state.was_dismissed || state.auto_removed {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                if !state.is_active {
                    state = vauchi_core::DemoContactState::new_active(
                        engine.vauchi().clock().unix_seconds(),
                    );
                    self.save_demo_state_engine(&state)?;
                }
                let contact = state.current_tip().map(|tip| {
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    card.into()
                });
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::GetDemoContact => {
                let state = self.load_demo_state_engine();
                let contact = if state.is_active {
                    state.current_tip().map(|tip| {
                        let card = vauchi_core::generate_demo_contact_card(&tip);
                        card.into()
                    })
                } else {
                    None
                };
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::GetDemoContactState => {
                let state = self.load_demo_state_engine();
                Ok(DomainCommandResult::DemoContactState {
                    state: crate::types::MobileDemoContactState {
                        is_active: state.is_active,
                        was_dismissed: state.was_dismissed,
                        auto_removed: state.auto_removed,
                        update_count: state.update_count,
                    },
                })
            }
            DomainCommand::IsDemoUpdateAvailable => {
                let state = self.load_demo_state_engine();
                Ok(DomainCommandResult::Bool {
                    value: state.is_update_due(engine.vauchi().clock().unix_seconds()),
                })
            }
            DomainCommand::TriggerDemoUpdate => {
                let mut state = self.load_demo_state_engine();
                if !state.is_active {
                    return Ok(DomainCommandResult::DemoContactOpt { contact: None });
                }
                let contact = if let Some(tip) =
                    state.advance_to_next_tip(engine.vauchi().clock().unix_seconds())
                {
                    self.save_demo_state_engine(&state)?;
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    Some(card.into())
                } else {
                    None
                };
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }
            DomainCommand::DismissDemoContact => {
                let mut state = self.load_demo_state_engine();
                state.dismiss();
                self.save_demo_state_engine(&state)?;
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::AutoRemoveDemoContact => {
                let mut state = self.load_demo_state_engine();
                let removed = if state.is_active {
                    state.auto_remove();
                    self.save_demo_state_engine(&state)?;
                    true
                } else {
                    false
                };
                Ok(DomainCommandResult::Bool { value: removed })
            }
            DomainCommand::RestoreDemoContact => {
                let mut state = self.load_demo_state_engine();
                state.restore();
                self.save_demo_state_engine(&state)?;
                let contact = state.current_tip().map(|tip| {
                    let card = vauchi_core::generate_demo_contact_card(&tip);
                    card.into()
                });
                Ok(DomainCommandResult::DemoContactOpt { contact })
            }

            // ── Contact Card + CRUD (B7 batch 10) ──
            //
            // Cache invalidation: own-card writes invalidate `MyInfo`;
            // contact writes invalidate `Contacts` + the specific
            // `ContactDetail { contact_id }` (where applicable);
            // archive writes invalidate `ArchivedContacts`. Reads
            // invalidate nothing.
            DomainCommand::GetOwnCard => {
                let card = engine
                    .vauchi()
                    .storage()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                Ok(DomainCommandResult::ContactCardPayload {
                    card: crate::types::MobileContactCard::from(&card),
                })
            }
            DomainCommand::AddField {
                field_type,
                label,
                value,
            } => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field = vauchi_core::ContactField::new(
                    field_type.into(),
                    &label,
                    &value,
                    engine.vauchi().clock().unix_seconds(),
                );
                card.add_field(field)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UpdateField { label, new_value } => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field '{label}' not found"),
                    })?
                    .id()
                    .to_string();
                card.update_field_value(&field_id, &new_value, storage.clock().unix_seconds())
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveField { label } => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = match card.fields().iter().find(|f| f.label() == label) {
                    Some(f) => f.id().to_string(),
                    None => return Ok(DomainCommandResult::Bool { value: false }),
                };
                card.remove_field(&field_id)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::SetDisplayName { name } => {
                // Route through `Vauchi::update_display_name` so the
                // identity's `display_name` column is updated in addition
                // to the own_card. The prior implementation mutated only
                // `own_card` and called `storage.save_own_card`, leaving
                // the identity column stale — which surfaced as the
                // Samsung S7 rename failure tracked by
                // `_private/docs/problems/2026-04-06-display-name-rename-fails/`.
                engine
                    .vauchi_mut()
                    .update_display_name(&name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetOwnAvatar { avatar_bytes } => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                card.set_avatar(avatar_bytes)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearOwnAvatar => {
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                card.clear_avatar();
                storage
                    .save_own_card(&card)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ListContacts => {
                let storage = engine.vauchi().storage();
                let contacts = storage
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::GetContact { id } => {
                let storage = engine.vauchi().storage();
                let contact = storage
                    .load_contact(&id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::ContactOpt {
                    contact: contact
                        .as_ref()
                        .map(|c| crate::mobile_contacts::enrich_contact(storage, c)),
                })
            }
            DomainCommand::SearchContacts { query } => {
                let storage = engine.vauchi().storage();
                let contacts =
                    storage
                        .search_contacts(&query)
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::ContactCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .len() as u32;
                Ok(DomainCommandResult::Count { value: count })
            }
            DomainCommand::RemoveContact { id } => {
                let removed = engine.vauchi().storage().delete_contact(&id).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                if removed {
                    engine.invalidate_screen(&AppScreen::Contacts);
                    engine.invalidate_screen(&AppScreen::ContactDetail {
                        contact_id: id.clone(),
                    });
                }
                Ok(DomainCommandResult::Bool { value: removed })
            }
            DomainCommand::SoftDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .soft_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UndoDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .undo_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HardDeleteImportedContact { id } => {
                engine
                    .vauchi()
                    .hard_delete_imported_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ArchiveContact { id } => {
                engine
                    .vauchi()
                    .archive_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UnarchiveContact { id } => {
                engine
                    .vauchi()
                    .unarchive_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ListArchivedContacts => {
                let storage = engine.vauchi().storage();
                let contacts =
                    engine
                        .vauchi()
                        .list_archived_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::HideContact { contact_id } => {
                engine
                    .vauchi()
                    .hide_contact(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UnhideContact { contact_id } => {
                engine
                    .vauchi()
                    .unhide_contact(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }

            // ── GDPR / Deletion + shred-status (B7 batch 3) ──
            DomainCommand::ExportGdprData => {
                let storage = engine.vauchi().storage();
                let export =
                    vauchi_core::api::export_all_data(storage).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let json_data =
                    serde_json::to_string_pretty(&export).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::GdprExport {
                    export: crate::types::MobileGdprExport {
                        json_data,
                        exported_at: export.exported_at,
                        version: export.version,
                    },
                })
            }
            DomainCommand::ScheduleIdentityDeletion => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                manager
                    .schedule_deletion()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::DeletionInfo {
                    info: crate::types::MobileDeletionInfo::from(&state),
                })
            }
            DomainCommand::CancelIdentityDeletion => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                manager.cancel_deletion().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ExecuteIdentityDeletion => {
                let revocation_count = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::DeletionManager::new(vauchi.storage());
                    let result =
                        manager
                            .execute_deletion(identity)
                            .map_err(|e| MobileError::Other {
                                detail: e.to_string(),
                            })?;
                    result.revocations.len() as u32
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Count {
                    value: revocation_count,
                })
            }
            DomainCommand::GetDeletionState => {
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                Ok(DomainCommandResult::DeletionInfo {
                    info: crate::types::MobileDeletionInfo::from(&state),
                })
            }
            DomainCommand::ShredStatus => {
                use crate::types::MobileShredStatus as MShred;
                let storage = engine.vauchi().storage();
                let manager = vauchi_core::api::DeletionManager::new(storage);
                let state = manager.deletion_state().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let status = match state {
                    vauchi_core::storage::DeletionState::None => MShred::None,
                    vauchi_core::storage::DeletionState::Scheduled { execute_at, .. } => {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        MShred::Scheduled {
                            remaining_secs: execute_at.saturating_sub(now),
                        }
                    }
                    vauchi_core::storage::DeletionState::Executed { .. } => MShred::Executed,
                    _ => MShred::None,
                };
                Ok(DomainCommandResult::ShredStatus { status })
            }
            DomainCommand::SoftShred => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let token = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager.soft_shred().map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredScheduled {
                    token: crate::types::MobileShredToken::from(&token),
                })
            }
            DomainCommand::CancelShred { token } => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
                {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .cancel_shred(core_token)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                }
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HardShred { token } => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let core_token = vauchi_core::api::ShredToken::from_created_at(token.created_at);
                let report = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let sender_id = identity.public_id();
                    let (mut purge, mut rev) = self.shred_senders(vauchi, &sender_id);
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .hard_shred(core_token, Some(&mut purge), Some(&mut rev))
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredCompleted {
                    report: crate::types::MobileShredReport::from(&report),
                })
            }
            DomainCommand::PanicShred => {
                let bridge = self.shred_keychain_bridge()?;
                let data_dir = self.shred_data_dir();
                let report = {
                    let vauchi = engine.vauchi();
                    let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                    let sender_id = identity.public_id();
                    let (mut purge, mut rev) = self.shred_senders(vauchi, &sender_id);
                    let manager = vauchi_core::api::ShredManager::new(
                        vauchi.storage(),
                        &bridge,
                        identity,
                        data_dir,
                    );
                    manager
                        .panic_shred(Some(&mut purge), Some(&mut rev))
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?
                };
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::ShredCompleted {
                    report: crate::types::MobileShredReport::from(&report),
                })
            }

            // ── Recovery leftovers (B7 batch 4) ──
            DomainCommand::VerifyRecoveryProof { proof_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryProof;

                let storage = engine.vauchi().storage();

                let proof_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&proof_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;
                proof.validate().map_err(|e| MobileError::InvalidInput {
                    field: String::new(),
                    detail: format!("Proof validation failed: {e}"),
                })?;

                let contacts = storage
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let contact_pks: std::collections::HashSet<[u8; 32]> = contacts
                    .iter()
                    .filter_map(|c| c.public_key().copied())
                    .collect();
                let known_voucher_count = proof
                    .vouchers()
                    .iter()
                    .filter(|v| contact_pks.contains(v.voucher_pk().as_bytes()))
                    .count();

                let (confidence, recommendation) = if known_voucher_count >= 2 {
                    (
                        "high".to_string(),
                        "Multiple contacts you know have vouched. Safe to accept.".to_string(),
                    )
                } else if known_voucher_count == 1 {
                    (
                        "medium".to_string(),
                        "One contact you know has vouched. Consider verifying in person."
                            .to_string(),
                    )
                } else {
                    (
                        "low".to_string(),
                        "No known contacts have vouched. Verify identity carefully before accepting."
                            .to_string(),
                    )
                };

                Ok(DomainCommandResult::RecoveryVerification {
                    verification: crate::types::MobileRecoveryVerification {
                        old_public_key: hex::encode(proof.old_pk()),
                        new_public_key: hex::encode(proof.new_pk()),
                        voucher_count: proof.voucher_count() as u32,
                        known_vouchers: known_voucher_count as u32,
                        confidence,
                        recommendation,
                    },
                })
            }
            DomainCommand::UploadGuardianEntries => {
                engine
                    .vauchi()
                    .upload_guardian_entries()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                // Guardian entries don't directly drive any visible
                // screen — they're a network-side artefact. No cache
                // invalidation needed.
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SaveRecoveryResponse {
                claim_id,
                contact_id,
                response,
                remind_at,
            } => {
                engine
                    .vauchi()
                    .save_recovery_response_action(&claim_id, &contact_id, &response, remind_at)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::Unit)
            }

            // ── Recovery-trust toggle + count (slice 32g-B) ──
            DomainCommand::TrustContactForRecovery { contact_id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                if contact.is_blocked() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Blocked contacts cannot be trusted for recovery".into(),
                    });
                }
                contact
                    .trust_for_recovery()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::UntrustContactForRecovery { contact_id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                contact
                    .untrust_for_recovery()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::TrustedContactCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .iter()
                    .filter(|c| c.is_recovery_trusted())
                    .count() as u32;
                Ok(DomainCommandResult::Count { value: count })
            }

            // ── Visibility Labels + Field Visibility (B7 batch 6) ──
            //
            // Cache invalidation: write-path commands invalidate the
            // Groups / GroupDetail / ContactDetail / ContactVisibility
            // screens. Reads invalidate nothing.
            DomainCommand::ListLabels => {
                let labels = engine.vauchi().storage().load_all_groups().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::Labels {
                    labels: labels
                        .iter()
                        .map(crate::types::MobileVisibilityLabel::from)
                        .collect(),
                })
            }
            DomainCommand::CreateLabel { name } => {
                let label = engine.vauchi().storage().create_group(&name).map_err(|e| {
                    MobileError::Other {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::Groups);
                Ok(DomainCommandResult::Label {
                    label: crate::types::MobileVisibilityLabel::from(&label),
                })
            }
            DomainCommand::GetLabel { label_id } => {
                let storage = engine.vauchi().storage();
                let label =
                    storage
                        .load_group(&label_id)
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })?;
                let mut detail = crate::types::MobileVisibilityLabelDetail::from(&label);
                let (rows, stale_count) =
                    crate::mobile_visibility::resolve_label_contacts(storage, &detail.contact_ids);
                detail.label_contacts = rows;
                detail.stale_reference_count = stale_count;
                Ok(DomainCommandResult::LabelDetail { detail })
            }
            DomainCommand::RenameLabel { label_id, new_name } => {
                engine
                    .vauchi()
                    .storage()
                    .rename_group(&label_id, &new_name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::DeleteLabel { label_id } => {
                engine
                    .vauchi()
                    .storage()
                    .delete_group(&label_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::AddContactToGroup {
                label_id,
                contact_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .add_contact_to_group(&label_id, &contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveContactFromGroup {
                label_id,
                contact_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .remove_contact_from_group(&label_id, &contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Groups);
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetGroupsForContact { contact_id } => {
                let labels = engine
                    .vauchi()
                    .storage()
                    .get_groups_for_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Labels {
                    labels: labels
                        .iter()
                        .map(crate::types::MobileVisibilityLabel::from)
                        .collect(),
                })
            }
            DomainCommand::SetGroupFieldVisibility {
                label_id,
                field_label,
                is_visible,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .set_group_field_visibility(&label_id, &field_id, is_visible)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::GroupDetail {
                    group_id: label_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::MyInfo);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactFieldOverride {
                contact_id,
                field_label,
                is_visible,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .save_contact_override(&contact_id, &field_id, is_visible)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::RemoveContactFieldOverride {
                contact_id,
                field_label,
            } => {
                let storage = engine.vauchi().storage();
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                storage
                    .delete_contact_override(&contact_id, &field_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::HideFieldFromContact {
                contact_id,
                field_label,
            } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                contact
                    .visibility_rules_mut()
                    .ok_or(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Visibility rules require an exchanged contact".into(),
                    })?
                    .set_nobody(&field_id);
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ShowFieldToContact {
                contact_id,
                field_label,
            } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                contact
                    .visibility_rules_mut()
                    .ok_or(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Visibility rules require an exchanged contact".into(),
                    })?
                    .set_everyone(&field_id);
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactVisibility {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::IsFieldVisibleToContact {
                contact_id,
                field_label,
            } => {
                let storage = engine.vauchi().storage();
                let contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                let card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                let field_id = card
                    .fields()
                    .iter()
                    .find(|f| f.label() == field_label)
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Field not found: {field_label}"),
                    })?
                    .id()
                    .to_string();
                let visible = contact
                    .visibility_rules()
                    .is_some_and(|r| r.can_see(&field_id, &contact_id));
                Ok(DomainCommandResult::Bool { value: visible })
            }
            DomainCommand::GetSuggestedLabels => {
                let values = vauchi_core::SUGGESTED_LABELS
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                Ok(DomainCommandResult::Strings { values })
            }

            // ── Passcode + Duress + Decoy (B7 batch 7) ──
            //
            // The legacy VauchiPlatform code calls `set_identity` per
            // method because each call opens a fresh Vauchi instance.
            // PlatformAppEngine's persistent Vauchi already holds the
            // identity from construction, so the wrappers can call the
            // password / duress methods directly without
            // re-installation.
            DomainCommand::SetupAppPassword { password } => {
                engine
                    .vauchi_mut()
                    .setup_app_password(&password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Lock);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetupDuressPassword { duress_password } => {
                engine
                    .vauchi_mut()
                    .setup_duress_password(&duress_password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DuressPin);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::Authenticate { password } => {
                let mode = engine.vauchi_mut().authenticate(&password).map_err(|e| {
                    MobileError::Other {
                        detail: e.to_string(),
                    }
                })?;
                let mapped = match mode {
                    vauchi_core::AuthMode::Normal => crate::types::MobileAuthMode::Normal,
                    vauchi_core::AuthMode::Duress => crate::types::MobileAuthMode::Duress,
                    _ => crate::types::MobileAuthMode::Normal,
                };
                Ok(DomainCommandResult::AuthMode { mode: mapped })
            }
            DomainCommand::IsPasswordEnabled => {
                let value =
                    engine
                        .vauchi()
                        .is_password_enabled()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::IsDuressEnabled => {
                let value =
                    engine
                        .vauchi()
                        .is_duress_enabled()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::DisableDuress => {
                engine
                    .vauchi_mut()
                    .disable_duress()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DuressPin);
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ConfigureDuressAlerts {
                contact_ids,
                message,
            } => {
                let settings = vauchi_core::DuressSettings {
                    alert_contact_ids: contact_ids,
                    alert_message: message,
                    include_location: false,
                };
                engine
                    .vauchi()
                    .save_duress_settings(&settings)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetDuressSettings => {
                let settings =
                    engine
                        .vauchi()
                        .load_duress_settings()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::DuressSettingsOpt {
                    settings: settings.map(|s| crate::types::MobileDuressSettings {
                        alert_contact_ids: s.alert_contact_ids,
                        alert_message: s.alert_message,
                        include_location: s.include_location,
                    }),
                })
            }
            DomainCommand::AddDecoyContact { name, card_json } => {
                let card: vauchi_core::ContactCard =
                    serde_json::from_str(&card_json).map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let id = format!(
                    "decoy-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_millis())
                        .unwrap_or(0)
                );
                engine
                    .vauchi()
                    .add_decoy_contact(&id, &name, &card)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Text { value: id })
            }
            DomainCommand::ListDecoyContacts => {
                let decoys =
                    engine
                        .vauchi()
                        .list_decoy_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::DecoyContacts {
                    contacts: decoys
                        .into_iter()
                        .map(
                            |(id, display_name, _card)| crate::types::MobileDecoyContact {
                                id,
                                display_name,
                            },
                        )
                        .collect(),
                })
            }
            DomainCommand::DeleteDecoyContact { id } => {
                engine
                    .vauchi()
                    .remove_decoy_contact(&id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::Privacy);
                Ok(DomainCommandResult::Unit)
            }

            // ── Sync / Delivery / Retry — read paths + simple writes (B7 batch 8) ──
            //
            // Cache invalidation: ManualRetry / DeleteRetry /
            // ClearPendingUpdatesForContact invalidate `DeliveryStatus`
            // (the user-visible delivery feed). Pure reads invalidate
            // nothing.
            DomainCommand::PendingUpdateCount => {
                let storage = engine.vauchi().storage();
                let contacts = storage
                    .list_contacts()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let mut total: u32 = 0;
                for contact in contacts {
                    let pending = storage.get_pending_updates(contact.id()).map_err(|e| {
                        MobileError::StorageError {
                            detail: e.to_string(),
                        }
                    })?;
                    total += pending.len() as u32;
                }
                Ok(DomainCommandResult::Count { value: total })
            }
            DomainCommand::GetDeliveryRecord { message_id } => {
                let record = engine
                    .vauchi()
                    .storage()
                    .get_delivery_record(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecordOpt {
                    record: record
                        .as_ref()
                        .map(crate::types::MobileDeliveryRecord::from),
                })
            }
            DomainCommand::GetAllDeliveryRecords => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_all_delivery_records()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetDeliveryRecordsForContact { recipient_id } => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_delivery_records_for_recipient(&recipient_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::CountFailedDeliveries => {
                let count = engine
                    .vauchi()
                    .storage()
                    .count_deliveries_by_status(&vauchi_core::storage::DeliveryStatus::Failed {
                        reason: String::new(),
                    })
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetFailedDeliveryRecords => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_delivery_records_by_status(&vauchi_core::storage::DeliveryStatus::Failed {
                        reason: String::new(),
                    })
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::ManualRetry { message_id } => {
                let storage = engine.vauchi().storage();
                let entry = storage.get_retry_entry(&message_id).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                if entry.is_none() {
                    return Ok(DomainCommandResult::Bool { value: false });
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                storage
                    .update_retry_next_time(&message_id, now)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DeliveryStatus);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::GetPendingDeliveries => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_pending_deliveries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliveryRecords {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetDeliveryCountByStatus { status } => {
                use vauchi_core::storage::DeliveryStatus;
                let core_status = match status {
                    crate::types::MobileDeliveryStatus::Queued => DeliveryStatus::Queued,
                    crate::types::MobileDeliveryStatus::Sent => DeliveryStatus::Sent,
                    crate::types::MobileDeliveryStatus::Stored => DeliveryStatus::Stored,
                    crate::types::MobileDeliveryStatus::Delivered => DeliveryStatus::Delivered,
                    crate::types::MobileDeliveryStatus::Expired => DeliveryStatus::Expired,
                    crate::types::MobileDeliveryStatus::Failed => DeliveryStatus::Failed {
                        reason: String::new(),
                    },
                };
                let count = engine
                    .vauchi()
                    .storage()
                    .count_deliveries_by_status(&core_status)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetDueRetries => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let entries = engine
                    .vauchi()
                    .storage()
                    .get_due_retries(now)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::RetryEntries {
                    entries: entries
                        .iter()
                        .map(crate::types::MobileRetryEntry::from)
                        .collect(),
                })
            }
            DomainCommand::GetRetriesForContact { contact_id } => {
                let entries = engine
                    .vauchi()
                    .storage()
                    .get_retry_entries_for_recipient(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::RetryEntries {
                    entries: entries
                        .iter()
                        .map(crate::types::MobileRetryEntry::from)
                        .collect(),
                })
            }
            DomainCommand::GetRetryCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .count_retry_entries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::DeleteRetry { message_id } => {
                let deleted = engine
                    .vauchi()
                    .storage()
                    .delete_retry_entry(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                if deleted {
                    engine.invalidate_screen(&AppScreen::DeliveryStatus);
                }
                Ok(DomainCommandResult::Bool { value: deleted })
            }
            DomainCommand::CalculateRetryBackoff { attempt } => {
                let queue = vauchi_core::storage::RetryQueue::new();
                Ok(DomainCommandResult::BackoffSeconds {
                    seconds: queue.backoff_seconds(attempt),
                })
            }
            DomainCommand::GetTotalPendingCount => {
                let count = engine
                    .vauchi()
                    .storage()
                    .count_all_pending_updates()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::IsOfflineQueueFull => {
                let queue = vauchi_core::storage::OfflineQueue::new();
                let value = queue.is_full(engine.vauchi().storage()).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::Bool { value })
            }
            DomainCommand::GetOfflineQueueCapacity => {
                let queue = vauchi_core::storage::OfflineQueue::new();
                let remaining = queue
                    .remaining_capacity(engine.vauchi().storage())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Count {
                    value: remaining as u32,
                })
            }
            DomainCommand::ClearPendingUpdatesForContact { contact_id } => {
                let count = engine
                    .vauchi()
                    .storage()
                    .delete_pending_updates_for_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::DeliveryStatus);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Count {
                    value: count as u32,
                })
            }
            DomainCommand::GetDeliverySummary { message_id } => {
                let summary = engine
                    .vauchi()
                    .storage()
                    .get_delivery_summary(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeliverySummary {
                    summary: crate::types::MobileDeliverySummary::from(&summary),
                })
            }
            DomainCommand::GetDeviceDeliveries { message_id } => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_device_deliveries_for_message(&message_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeviceDeliveries {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeviceDeliveryRecord::from)
                        .collect(),
                })
            }
            DomainCommand::GetPendingDeviceDeliveries => {
                let records = engine
                    .vauchi()
                    .storage()
                    .get_pending_device_deliveries()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DeviceDeliveries {
                    records: records
                        .iter()
                        .map(crate::types::MobileDeviceDeliveryRecord::from)
                        .collect(),
                })
            }

            // ── Identity reads + Onboarding helpers (B7 batch 9) ──
            DomainCommand::CreateIdentity { display_name } => {
                engine
                    .vauchi_mut()
                    .create_identity(&display_name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetPublicId => {
                let value = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?
                    .public_id();
                Ok(DomainCommandResult::Text { value })
            }
            DomainCommand::GetDisplayName => {
                let value = engine
                    .vauchi()
                    .storage()
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?
                    .display_name()
                    .to_string();
                Ok(DomainCommandResult::Text { value })
            }
            DomainCommand::GetOwnFingerprint => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let hex = hex::encode(identity.signing_public_key());
                let formatted = hex
                    .chars()
                    .collect::<Vec<_>>()
                    .chunks(4)
                    .map(|c| c.iter().collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_uppercase();
                Ok(DomainCommandResult::Text { value: formatted })
            }
            DomainCommand::DisplayNameSuggestions { full_name } => {
                Ok(DomainCommandResult::Strings {
                    values: vauchi_core::display_name_suggestions(&full_name),
                })
            }
            DomainCommand::ResetOnboarding => {
                let storage = engine.vauchi().storage();
                let mut progress = storage.load_or_create_onboarding_progress().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                progress.reset(engine.vauchi().clock().unix_seconds());
                storage.save_onboarding_progress(&progress).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                engine.invalidate_screen(&AppScreen::Onboarding);
                Ok(DomainCommandResult::Unit)
            }

            // ── Contact Verification + Duplicates + Notes + Misc (B7 batch 11) ──
            DomainCommand::VerifyContact { id } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {id}"),
                    })?;
                contact
                    .mark_fingerprint_verified()
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetProposalTrusted {
                contact_id,
                trusted,
            } => {
                let storage = engine.vauchi().storage();
                let mut contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::Other {
                        detail: format!("Contact not found: {contact_id}"),
                    })?;
                contact
                    .set_proposal_trusted(trusted)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: e.to_string(),
                    })?;
                storage
                    .save_contact(&contact)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::FindDuplicates => {
                let pairs = engine
                    .vauchi()
                    .find_duplicates()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::DuplicatePairs {
                    pairs: pairs
                        .into_iter()
                        .map(|p| crate::types::MobileDuplicatePair {
                            id1: p.id1,
                            id2: p.id2,
                            similarity: p.similarity,
                        })
                        .collect(),
                })
            }
            DomainCommand::DismissDuplicate { id1, id2 } => {
                engine
                    .vauchi()
                    .dismiss_duplicate(&id1, &id2)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDuplicates);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactNote { contact_id, note } => {
                engine
                    .vauchi()
                    .storage()
                    .save_personal_notes(&contact_id, note.as_bytes())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactNote { contact_id } => {
                let bytes = engine
                    .vauchi()
                    .storage()
                    .load_personal_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::StringOpt {
                    value: bytes.and_then(|b| String::from_utf8(b).ok()),
                })
            }
            DomainCommand::DeleteContactNote { contact_id } => {
                engine
                    .vauchi()
                    .storage()
                    .delete_personal_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactFieldNote {
                contact_id,
                field_id,
                note,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .save_contact_field_note(&contact_id, &field_id, note.as_bytes())
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactFieldNotes { contact_id } => {
                let map = engine
                    .vauchi()
                    .storage()
                    .load_contact_field_notes(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let mut notes: Vec<crate::types::MobileFieldNote> = map
                    .into_iter()
                    .filter_map(|(field_id, bytes)| {
                        String::from_utf8(bytes)
                            .ok()
                            .map(|note| crate::types::MobileFieldNote { field_id, note })
                    })
                    .collect();
                notes.sort_by(|a, b| a.field_id.cmp(&b.field_id));
                Ok(DomainCommandResult::FieldNotes { notes })
            }
            DomainCommand::DeleteContactFieldNote {
                contact_id,
                field_id,
            } => {
                engine
                    .vauchi()
                    .storage()
                    .delete_contact_field_note(&contact_id, &field_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactNickname { contact_id, name } => {
                engine
                    .vauchi()
                    .set_contact_nickname(&contact_id, &name)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearContactNickname { contact_id } => {
                engine
                    .vauchi()
                    .clear_contact_nickname(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetContactCustomAvatar { contact_id, data } => {
                engine
                    .vauchi()
                    .set_contact_custom_avatar(&contact_id, &data)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ClearContactCustomAvatar { contact_id } => {
                engine
                    .vauchi()
                    .clear_contact_custom_avatar(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::GetContactCustomAvatar { contact_id } => {
                let data = engine
                    .vauchi()
                    .get_contact_custom_avatar(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::AvatarOpt { data })
            }
            DomainCommand::SearchSocialNetworks { query } => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                let networks = registry
                    .search(&query)
                    .iter()
                    .map(|sn| crate::types::MobileSocialNetwork {
                        id: sn.id().to_string(),
                        display_name: sn.display_name().to_string(),
                        url_template: sn.profile_url_template().to_string(),
                    })
                    .collect();
                Ok(DomainCommandResult::SocialNetworks { networks })
            }
            DomainCommand::GetProfileUrl {
                network_id,
                username,
            } => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                Ok(DomainCommandResult::StringOpt {
                    value: registry.profile_url(&network_id, &username),
                })
            }
            DomainCommand::ListHiddenContacts => {
                let storage = engine.vauchi().storage();
                let contacts =
                    engine
                        .vauchi()
                        .list_hidden_contacts()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }
            DomainCommand::ContactDetailFooterActionId { contact_id } => {
                let contact = engine
                    .vauchi()
                    .storage()
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: "contact_id".into(),
                        detail: format!("contact not found: {contact_id}"),
                    })?;
                let value = vauchi_app::ui::contact_detail_footer_action_id(contact.is_imported())
                    .to_string();
                Ok(DomainCommandResult::Text { value })
            }

            // ── Backup + Import (B7 batch 12) ──
            DomainCommand::ExportBackup { password } => {
                let backup_hex =
                    engine
                        .vauchi()
                        .export_backup(&password)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                use base64::Engine;
                let bytes = hex::decode(&backup_hex).map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(DomainCommandResult::Text { value: encoded })
            }
            DomainCommand::ImportBackup {
                backup_data,
                password,
            } => {
                if engine.vauchi().identity().is_some() {
                    return Err(MobileError::Other {
                        detail: "Already initialized".to_string(),
                    });
                }
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&backup_data)
                    .map_err(|_| MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid base64".to_string(),
                    })?;
                let backup_hex = hex::encode(&bytes);
                engine
                    .vauchi_mut()
                    .import_backup(&backup_hex, &password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ExportFullBackup { password } => {
                let backup_hex = engine.vauchi().export_full_backup(&password).map_err(|e| {
                    MobileError::Other {
                        detail: e.to_string(),
                    }
                })?;
                use base64::Engine;
                let bytes = hex::decode(&backup_hex).map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                Ok(DomainCommandResult::Text { value: encoded })
            }
            DomainCommand::ImportFullBackup {
                backup_data,
                password,
            } => {
                use base64::Engine;
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&backup_data)
                    .map_err(|_| MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid base64".to_string(),
                    })?;
                let backup_hex = hex::encode(&bytes);
                engine
                    .vauchi_mut()
                    .import_full_backup(&backup_hex, &password)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_all();
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::ImportContactsFromVcf { data } => {
                let result = engine
                    .vauchi()
                    .import_contacts_from_vcf(&data)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::ImportResult {
                    result: crate::mobile_import::MobileImportResult {
                        imported: result.imported as u32,
                        skipped: result.skipped as u32,
                        warnings: result.warnings.into_iter().map(Into::into).collect(),
                    },
                })
            }

            // ── Search + Display Prefs + Merge (B7 batch 14) ──
            // SearchContacts arm already provided by batch 10 above.
            DomainCommand::SetDisplayNamePreference {
                contact_id,
                pref_json,
            } => {
                let pref: vauchi_core::DisplayNamePreference = serde_json::from_str(&pref_json)
                    .map_err(|e| MobileError::InvalidInput {
                        field: "pref_json".into(),
                        detail: format!("Invalid preference JSON: {e}"),
                    })?;
                engine
                    .vauchi()
                    .set_display_name_preference(&contact_id, pref)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::SetAvatarPreference {
                contact_id,
                pref_json,
            } => {
                let pref: vauchi_core::AvatarPreference = serde_json::from_str(&pref_json)
                    .map_err(|e| MobileError::InvalidInput {
                        field: "pref_json".into(),
                        detail: format!("Invalid preference JSON: {e}"),
                    })?;
                engine
                    .vauchi()
                    .set_avatar_preference(&contact_id, pref)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: contact_id.clone(),
                });
                engine.invalidate_screen(&AppScreen::Contacts);
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::MergeContacts {
                primary_id,
                secondary_id,
            } => {
                let merged = engine
                    .vauchi()
                    .merge_contacts(&primary_id, &secondary_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let storage = engine.vauchi().storage();
                let contact = crate::mobile_contacts::enrich_contact(storage, &merged);
                engine.invalidate_screen(&AppScreen::Contacts);
                engine.invalidate_screen(&AppScreen::ContactDuplicates);
                engine.invalidate_screen(&AppScreen::ContactDetail {
                    contact_id: primary_id.clone(),
                });
                Ok(DomainCommandResult::ContactSingle { contact })
            }

            // ── Onboarding state ops (B7 batch 16) ──
            DomainCommand::GetOnboardingProgress => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }
            DomainCommand::CurrentOnboardingStep => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::OnboardingStep {
                    step: progress.current_step().into(),
                })
            }
            DomainCommand::IsOnboardingComplete => {
                let progress = engine
                    .vauchi()
                    .storage()
                    .load_or_create_onboarding_progress()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Bool {
                    value: progress.is_complete(),
                })
            }
            DomainCommand::AdvanceOnboarding => {
                let storage = engine.vauchi().storage();
                let mut progress = storage.load_or_create_onboarding_progress().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                progress.advance(engine.vauchi().clock().unix_seconds());
                storage.save_onboarding_progress(&progress).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }
            DomainCommand::SkipOnboardingStep => {
                let storage = engine.vauchi().storage();
                let mut progress = storage.load_or_create_onboarding_progress().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                progress.skip_step(engine.vauchi().clock().unix_seconds());
                storage.save_onboarding_progress(&progress).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                Ok(DomainCommandResult::OnboardingProgress {
                    progress: crate::types::MobileOnboardingProgress::from(&progress),
                })
            }

            // ── Contact display options + paginated/archived lists (B7 batch 17) ──
            DomainCommand::GetContactDisplayOptions { contact_id } => {
                let opts = engine
                    .vauchi()
                    .get_contact_display_options(&contact_id)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                let names = opts
                    .names
                    .into_iter()
                    .map(|n| {
                        Ok(crate::types::MobileNameOption {
                            source: serde_json::to_string(&n.source).map_err(|e| {
                                MobileError::Other {
                                    detail: format!("Serialize name source: {e}"),
                                }
                            })?,
                            name: n.name,
                            is_primary: n.is_primary,
                        })
                    })
                    .collect::<Result<Vec<_>, MobileError>>()?;
                let avatars = opts
                    .avatars
                    .into_iter()
                    .map(|a| {
                        Ok(crate::types::MobileAvatarOption {
                            source: serde_json::to_string(&a.source).map_err(|e| {
                                MobileError::Other {
                                    detail: format!("Serialize avatar source: {e}"),
                                }
                            })?,
                            has_data: a.has_data,
                            is_primary: a.is_primary,
                        })
                    })
                    .collect::<Result<Vec<_>, MobileError>>()?;
                let active_name_preference = serde_json::to_string(&opts.active_name_preference)
                    .map_err(|e| MobileError::Other {
                        detail: format!("Serialize name pref: {e}"),
                    })?;
                let active_avatar_preference =
                    serde_json::to_string(&opts.active_avatar_preference).map_err(|e| {
                        MobileError::Other {
                            detail: format!("Serialize avatar pref: {e}"),
                        }
                    })?;
                Ok(DomainCommandResult::ContactDisplayOptions {
                    options: crate::types::MobileContactDisplayOptions {
                        names,
                        avatars,
                        active_name_preference,
                        active_avatar_preference,
                    },
                })
            }
            DomainCommand::ListContactsPaginated { offset, limit } => {
                let storage = engine.vauchi().storage();
                let contacts = storage
                    .list_contacts_paginated(offset as usize, limit as usize)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                Ok(DomainCommandResult::Contacts {
                    contacts: crate::mobile_contacts::enrich_contacts_batch(storage, &contacts),
                })
            }

            // ── Contact detail view state + social registry (B7 batch 19) ──
            DomainCommand::ContactDetailViewState { contact_id } => {
                use vauchi_app::i18n::Locale;
                use vauchi_app::ui::{
                    ReciprocityBannerKind, reciprocity_banner, show_recovery_trusted_indicator,
                    show_verified_badge, verify_button_visible,
                };
                let storage = engine.vauchi().storage();
                let contact = storage
                    .load_contact(&contact_id)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or_else(|| MobileError::InvalidInput {
                        field: "contact_id".to_string(),
                        detail: format!("contact not found: {contact_id}"),
                    })?;

                let mut badges = Vec::new();
                if show_verified_badge(contact.is_fingerprint_verified()) {
                    badges.push(crate::mobile_contact_detail::MobileContactDetailBadge::Verified);
                }
                if show_recovery_trusted_indicator(contact.is_recovery_trusted()) {
                    badges.push(
                        crate::mobile_contact_detail::MobileContactDetailBadge::RecoveryTrusted,
                    );
                }

                let mut banners = Vec::new();
                if let Some(kind) = reciprocity_banner(contact.reciprocity(0)) {
                    banners.push(match kind {
                        ReciprocityBannerKind::Pending => {
                            crate::mobile_contact_detail::MobileContactDetailBanner::ReciprocityPending {
                                label: "Waiting for them to share their info".to_string(),
                            }
                        }
                        ReciprocityBannerKind::Unreciprocated => {
                            crate::mobile_contact_detail::MobileContactDetailBanner::ReciprocityUnreciprocated {
                                label: "They haven't shared their info".to_string(),
                            }
                        }
                    });
                }

                let mut actions = Vec::new();
                if verify_button_visible(contact.is_fingerprint_verified(), contact.trust_level()) {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Verify);
                }
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::ToggleRecoveryTrust {
                        currently_trusted: contact.is_recovery_trusted(),
                    },
                );
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::ToggleHidden {
                        currently_hidden: contact.is_hidden(),
                    },
                );
                actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Edit);
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::VerifyFingerprint,
                );
                actions.push(
                    crate::mobile_contact_detail::MobileContactDetailAction::PreviewAs {
                        contact_id: contact_id.clone(),
                    },
                );
                if contact.is_imported() {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Delete);
                } else {
                    actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Archive);
                }
                actions.push(crate::mobile_contact_detail::MobileContactDetailAction::Back);

                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let added_time_display = crate::mobile_contact_detail::compute_added_time_display(
                    &contact,
                    now,
                    Locale::English,
                );

                Ok(DomainCommandResult::ContactDetailView {
                    state: crate::mobile_contact_detail::MobileContactDetailViewState {
                        badges,
                        banners,
                        actions,
                        added_time_display,
                    },
                })
            }
            DomainCommand::ListSocialNetworks => {
                let registry = vauchi_core::SocialNetworkRegistry::with_defaults();
                let networks = registry
                    .all()
                    .iter()
                    .map(|sn| crate::types::MobileSocialNetwork {
                        id: sn.id().to_string(),
                        display_name: sn.display_name().to_string(),
                        url_template: sn.profile_url_template().to_string(),
                    })
                    .collect();
                Ok(DomainCommandResult::SocialNetworks { networks })
            }

            // ── Multipart QR encoding (B7 batch 20) ──
            DomainCommand::EncodeMultipartQr { data } => {
                drop(engine); // stateless: no engine state needed
                let frames = crate::multipart_qr::encode_multipart(&data, 1800);
                Ok(DomainCommandResult::Strings { values: frames })
            }

            // ── Certificate pinning (B7 batch 21) ──
            DomainCommand::SetPinnedCertificate { cert_pem } => {
                drop(engine);
                let path = self.cert_pin_path_engine();
                if cert_pem.is_empty() {
                    // Empty string clears the pin: remove the sidecar file.
                    // Ignore NotFound — already-cleared is idempotent.
                    if let Err(e) = std::fs::remove_file(&path)
                        && e.kind() != std::io::ErrorKind::NotFound
                    {
                        return Err(MobileError::StorageError {
                            detail: e.to_string(),
                        });
                    }
                } else {
                    std::fs::write(&path, cert_pem).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                }
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::IsCertificatePinningEnabled => {
                drop(engine);
                Ok(DomainCommandResult::Bool {
                    value: self.cert_pin_path_engine().exists(),
                })
            }

            // ── Device linking — Track B Tier 2 (B7 batch 22) ──
            DomainCommand::IsPrimaryDevice => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                Ok(DomainCommandResult::Bool {
                    value: identity.device_info().device_index() == 0,
                })
            }
            DomainCommand::GetDeviceCount => {
                let storage = engine.vauchi().storage();
                let count =
                    match storage
                        .load_device_registry()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })? {
                        Some(r) => r.device_count() as u32,
                        None => 1,
                    };
                Ok(DomainCommandResult::Count { value: count })
            }
            DomainCommand::GetDevices => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let storage = engine.vauchi().storage();

                let registry =
                    match storage
                        .load_device_registry()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })? {
                        Some(r) => r,
                        None => {
                            let device_info = identity.device_info();
                            return Ok(DomainCommandResult::Devices {
                                devices: vec![crate::types::MobileDeviceInfo {
                                    device_index: device_info.device_index(),
                                    device_name: device_info.device_name().to_string(),
                                    is_current: true,
                                    is_active: true,
                                    public_key_prefix: hex::encode(&device_info.device_id()[..8]),
                                    created_at: device_info.created_at(),
                                }],
                            });
                        }
                    };

                let current_device_id = identity.device_info().device_id();
                let devices = registry
                    .all_devices()
                    .iter()
                    .enumerate()
                    .map(
                        |(idx, d): (usize, &vauchi_core::identity::RegisteredDevice)| {
                            crate::types::MobileDeviceInfo {
                                device_index: idx as u32,
                                device_name: d.device_name.clone(),
                                is_current: d.device_id == *current_device_id,
                                is_active: d.is_active(),
                                public_key_prefix: hex::encode(&d.device_id[..8]),
                                created_at: d.created_at,
                            }
                        },
                    )
                    .collect();
                Ok(DomainCommandResult::Devices { devices })
            }
            DomainCommand::UnlinkDevice { device_index } => {
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;
                let storage = engine.vauchi().storage();

                let mut registry =
                    match storage
                        .load_device_registry()
                        .map_err(|e| MobileError::StorageError {
                            detail: e.to_string(),
                        })? {
                        Some(r) => r,
                        None => return Ok(DomainCommandResult::Bool { value: false }),
                    };

                let devices = registry.all_devices();
                if device_index as usize >= devices.len() {
                    return Ok(DomainCommandResult::Bool { value: false });
                }

                let device_id = devices[device_index as usize].device_id;
                if device_id == *identity.device_info().device_id() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Cannot unlink the current device".into(),
                    });
                }

                let result = match registry.revoke_device(
                    &device_id,
                    identity.signing_keypair(),
                    storage.clock().unix_seconds(),
                ) {
                    Ok(()) => {
                        storage.save_device_registry(&registry).map_err(|e| {
                            MobileError::StorageError {
                                detail: e.to_string(),
                            }
                        })?;
                        true
                    }
                    Err(_) => false,
                };

                engine.invalidate_screen(&AppScreen::DeviceManagement);
                engine.invalidate_screen(&AppScreen::DeviceLinking);
                Ok(DomainCommandResult::Bool { value: result })
            }
            DomainCommand::GenerateDeviceLinkQr => {
                use vauchi_core::exchange::device_link::DeviceLinkQR;

                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;

                let qr = DeviceLinkQR::generate(
                    identity,
                    vauchi_core::clock::SystemClock::shared().unix_seconds(),
                );
                Ok(DomainCommandResult::DeviceLinkData {
                    data: crate::types::MobileDeviceLinkData {
                        qr_data: qr.to_data_string(),
                        identity_public_key: hex::encode(identity.signing_public_key()),
                        timestamp: qr.timestamp(),
                        expires_at: qr.expires_at(),
                    },
                })
            }
            DomainCommand::ParseDeviceLinkQr { qr_data } => {
                use vauchi_core::exchange::device_link::DeviceLinkQR;

                let qr = DeviceLinkQR::from_data_string(&qr_data).map_err(|_| {
                    MobileError::InvalidInput {
                        field: "qr".into(),
                        detail: "Invalid QR code".into(),
                    }
                })?;

                Ok(DomainCommandResult::DeviceLinkInfo {
                    info: crate::types::MobileDeviceLinkInfo {
                        identity_public_key: hex::encode(qr.identity_public_key()),
                        timestamp: qr.timestamp(),
                        is_expired: qr
                            .is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
                    },
                })
            }
            DomainCommand::ParseRecoveryClaim { claim_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryClaim;

                let claim_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&claim_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let claim = RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid claim: {e}"),
                    }
                })?;

                Ok(DomainCommandResult::RecoveryClaim {
                    claim: crate::types::MobileRecoveryClaim {
                        old_public_key: hex::encode(claim.old_pk()),
                        new_public_key: hex::encode(claim.new_pk()),
                        claim_data: claim_b64,
                        is_expired: claim
                            .is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()),
                    },
                })
            }
            DomainCommand::GetRecoveryProof => {
                use base64::Engine as _;
                use vauchi_core::recovery::RecoveryProof;

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Ok(DomainCommandResult::StringOpt { value: None });
                }

                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                let value = if proof.voucher_count() >= proof.threshold() as usize {
                    let bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                    Some(base64::engine::general_purpose::STANDARD.encode(bytes))
                } else {
                    None
                };
                Ok(DomainCommandResult::StringOpt { value })
            }
            DomainCommand::GetRecoveryStatus => {
                use vauchi_core::recovery::RecoveryProof;

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Ok(DomainCommandResult::OptionalRecoveryProgress { progress: None });
                }

                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                Ok(DomainCommandResult::OptionalRecoveryProgress {
                    progress: Some(crate::types::MobileRecoveryProgress {
                        old_public_key: hex::encode(proof.old_pk()),
                        new_public_key: hex::encode(proof.new_pk()),
                        vouchers_collected: proof.voucher_count() as u32,
                        vouchers_needed: proof.threshold(),
                        is_complete: proof.voucher_count() >= proof.threshold() as usize,
                    }),
                })
            }
            DomainCommand::CreateRecoveryVoucher { claim_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

                // `engine` is already locked by the dispatch entry point;
                // re-locking the non-reentrant mutex would deadlock.
                let identity = engine
                    .vauchi()
                    .identity()
                    .ok_or_else(|| MobileError::Other {
                        detail: "Identity not initialized".into(),
                    })?;

                let claim_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&claim_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let claim = RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid claim: {e}"),
                    }
                })?;

                if claim.is_expired(vauchi_core::clock::SystemClock::shared().unix_seconds()) {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Claim has expired".into(),
                    });
                }

                let voucher =
                    RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair(), None, 0)
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                let voucher_data =
                    base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

                Ok(DomainCommandResult::RecoveryVoucher {
                    voucher: crate::types::MobileRecoveryVoucher {
                        voucher_public_key: hex::encode(voucher.voucher_pk()),
                        voucher_data,
                    },
                })
            }
            DomainCommand::AddRecoveryVoucher { voucher_b64 } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryProof, RecoveryVoucher};

                // `engine` already locked by dispatch entry — do not re-lock.
                let voucher_bytes = base64::engine::general_purpose::STANDARD
                    .decode(&voucher_b64)
                    .map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid base64: {e}"),
                    })?;
                let voucher = RecoveryVoucher::from_bytes(&voucher_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid voucher: {e}"),
                    }
                })?;

                if !voucher.verify() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "Invalid voucher signature".into(),
                    });
                }

                let proof_path = self.recovery_proof_path();
                if !proof_path.exists() {
                    return Err(MobileError::InvalidInput {
                        field: String::new(),
                        detail: "No recovery in progress".into(),
                    });
                }
                let proof_bytes =
                    std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                let mut proof = RecoveryProof::from_bytes(&proof_bytes).map_err(|e| {
                    MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid proof: {e}"),
                    }
                })?;

                let contacts = engine.vauchi().storage().list_contacts().map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;
                let trusted_keys: std::collections::HashSet<[u8; 32]> = contacts
                    .iter()
                    .filter(|c| c.is_recovery_trusted())
                    .filter_map(|c| c.public_key().copied())
                    .collect();

                match proof.add_voucher_trusted(voucher, &trusted_keys) {
                    Ok(()) => {}
                    Err(vauchi_core::recovery::RecoveryError::UntrustedVoucher) => {
                        return Err(MobileError::InvalidInput {
                            field: String::new(),
                            detail: "Voucher is from an untrusted contact. Only contacts marked as recovery-trusted can provide valid vouchers.".into(),
                        });
                    }
                    Err(e) => {
                        return Err(MobileError::InvalidInput {
                            field: String::new(),
                            detail: format!("Cannot add voucher: {e}"),
                        });
                    }
                }

                let updated_bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                std::fs::write(&proof_path, updated_bytes).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;

                let progress = crate::types::MobileRecoveryProgress {
                    old_public_key: hex::encode(proof.old_pk()),
                    new_public_key: hex::encode(proof.new_pk()),
                    vouchers_collected: proof.voucher_count() as u32,
                    vouchers_needed: proof.threshold(),
                    is_complete: proof.voucher_count() >= proof.threshold() as usize,
                };

                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::RecoveryProgress { progress })
            }
            DomainCommand::CreateRecoveryClaim { old_pk_hex } => {
                use base64::Engine as _;
                use vauchi_core::recovery::{RecoveryClaim, RecoveryProof};

                // `engine` already locked by dispatch entry — do not re-lock.
                // Scope the identity borrow so the later `invalidate_screen`
                // mutable borrows are free.
                let new_pk = {
                    let identity =
                        engine
                            .vauchi()
                            .identity()
                            .ok_or_else(|| MobileError::Other {
                                detail: "Identity not initialized".into(),
                            })?;
                    *identity.signing_public_key()
                };

                let old_pk_bytes =
                    hex::decode(&old_pk_hex).map_err(|e| MobileError::InvalidInput {
                        field: String::new(),
                        detail: format!("Invalid hex: {e}"),
                    })?;
                let old_pk: [u8; 32] =
                    old_pk_bytes
                        .try_into()
                        .map_err(|_| MobileError::InvalidInput {
                            field: String::new(),
                            detail: "Public key must be 32 bytes".into(),
                        })?;

                let now = vauchi_core::clock::SystemClock::shared().unix_seconds();
                let claim = RecoveryClaim::new(old_pk, new_pk, now);

                // Persist a `RecoveryProof` shell beside the database —
                // mirrors the legacy file layout. Threshold default 3.
                let proof = RecoveryProof::new(old_pk, new_pk, 3, now);
                let proof_bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
                std::fs::write(self.recovery_proof_path(), proof_bytes).map_err(|e| {
                    MobileError::StorageError {
                        detail: e.to_string(),
                    }
                })?;

                let claim_data = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());
                let result = crate::types::MobileRecoveryClaim {
                    old_public_key: old_pk_hex,
                    new_public_key: hex::encode(new_pk),
                    claim_data,
                    is_expired: claim.is_expired(now),
                };

                engine.invalidate_screen(&AppScreen::Recovery);
                engine.invalidate_screen(&AppScreen::RecoveryHelp);
                Ok(DomainCommandResult::RecoveryClaim { claim: result })
            }
            DomainCommand::ConfigureEmergencyBroadcast {
                contact_ids,
                message,
                include_location,
            } => {
                engine
                    .vauchi_mut()
                    .configure_emergency_broadcast(contact_ids, message, include_location)
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Bool { value: true })
            }
            DomainCommand::SendEmergencyBroadcast => {
                let result = engine
                    .vauchi_mut()
                    .send_emergency_broadcast()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::BroadcastResult {
                    result: crate::types::MobileBroadcastResult {
                        sent: result.sent as u32,
                        total: result.total as u32,
                    },
                })
            }
            DomainCommand::GetEmergencyConfig => {
                let config =
                    engine
                        .vauchi()
                        .load_emergency_config()
                        .map_err(|e| MobileError::Other {
                            detail: e.to_string(),
                        })?;
                Ok(DomainCommandResult::OptionalEmergencyConfig {
                    config: config.map(|c| crate::types::MobileEmergencyConfig {
                        trusted_contact_ids: c.trusted_contact_ids,
                        message: c.message,
                        include_location: c.include_location,
                    }),
                })
            }
            DomainCommand::DisableEmergencyBroadcast => {
                engine
                    .vauchi_mut()
                    .delete_emergency_config()
                    .map_err(|e| MobileError::Other {
                        detail: e.to_string(),
                    })?;
                engine.invalidate_screen(&AppScreen::Settings);
                engine.invalidate_screen(&AppScreen::EmergencyShred);
                Ok(DomainCommandResult::Bool { value: true })
            }
        }
    }
}

impl PlatformAppEngine {
    /// File path holding the in-progress recovery proof, parallel to
    /// the SQLite database. Mirrors the legacy `VauchiPlatform` layout
    /// so both surfaces observe the same on-disk state during the
    /// Phase-C migration window.
    fn recovery_proof_path(&self) -> std::path::PathBuf {
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

    fn load_aha_tracker_engine(&self) -> vauchi_core::AhaMomentTracker {
        let path = self.aha_moments_path_engine();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::AhaMomentTracker::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::AhaMomentTracker::new()
        }
    }

    fn save_aha_tracker_engine(
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

    fn load_demo_state_engine(&self) -> vauchi_core::DemoContactState {
        let path = self.demo_contact_path_engine();
        if let Ok(data) = std::fs::read_to_string(&path) {
            vauchi_core::DemoContactState::from_json(&data).unwrap_or_default()
        } else {
            vauchi_core::DemoContactState::default()
        }
    }

    fn save_demo_state_engine(
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
    fn cert_pin_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".cert_pin")
    }

    /// Feature-gated content-update check (B7 batch 2). Returns
    /// `MobileUpdateStatus::Disabled` when the `content-updates` Cargo
    /// feature is off — matches legacy `VauchiPlatform::check_content_updates`.
    fn check_content_updates_dispatch(&self) -> crate::content::MobileUpdateStatus {
        #[cfg(feature = "content-updates")]
        {
            self.check_content_updates_impl_engine()
        }
        #[cfg(not(feature = "content-updates"))]
        {
            crate::content::MobileUpdateStatus::Disabled
        }
    }

    /// Feature-gated content-update apply (B7 batch 2). Mirrors the
    /// legacy `VauchiPlatform::apply_content_updates` semantics —
    /// returns `Disabled` when the feature is off.
    fn apply_content_updates_dispatch(&self) -> crate::content::MobileApplyResult {
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
        use crate::content::MobileUpdateStatus;
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
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                };
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileUpdateStatus::CheckFailed {
                    error: e.to_string(),
                };
            }
        };

        rt.block_on(async { manager.check_for_updates().await.into() })
    }

    fn apply_content_updates_impl_engine(&self) -> crate::content::MobileApplyResult {
        use crate::content::{MobileApplyFailure, MobileApplyResult, MobileContentType};
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
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                };
            }
        };

        let rt: tokio::runtime::Runtime = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                return MobileApplyResult::Error {
                    error: e.to_string(),
                };
            }
        };

        rt.block_on(async {
            match manager.apply_updates().await {
                Ok(result) => match result {
                    vauchi_app::content::ApplyResult::NoUpdates => MobileApplyResult::NoUpdates,
                    vauchi_app::content::ApplyResult::Disabled => MobileApplyResult::Disabled,
                    vauchi_app::content::ApplyResult::Applied { applied, failed } => {
                        MobileApplyResult::Applied {
                            applied: applied.into_iter().map(MobileContentType::from).collect(),
                            failed: failed
                                .into_iter()
                                .map(|(ct, err)| MobileApplyFailure {
                                    content_type: MobileContentType::from(ct),
                                    error: err,
                                })
                                .collect(),
                        }
                    }
                    _ => MobileApplyResult::Error {
                        error: "unknown apply result".to_string(),
                    },
                },
                Err(e) => MobileApplyResult::Error {
                    error: e.to_string(),
                },
            }
        })
    }
}
