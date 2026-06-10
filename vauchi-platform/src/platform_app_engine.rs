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
use crate::platform_app_engine_internals::self_heal_post_auth;

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
    pub(crate) engine: Arc<Mutex<AppEngine>>,
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
    pub(crate) storage_path: PathBuf,
    /// Platform keychain for crypto-shred `DomainCommand`s (B7). Set
    /// post-construction via `set_platform_keychain`, mirroring
    /// `VauchiPlatform`'s slot. `None` until the frontend wires it.
    pub(crate) platform_keychain: Mutex<Option<Arc<dyn crate::MobilePlatformKeychain>>>,
    /// Relay URL retained for building shred purge/revocation senders
    /// (B7 Phase 1b) — PAE can't reopen a fresh relay `Vauchi` (no stored
    /// storage key), so hard/panic shred builds senders off the live engine
    /// `Vauchi` + this URL.
    pub(crate) relay_url: String,
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
                    // A GATT peripheral never scans, so it emits no
                    // `BleDeviceDiscovered` and the discovery branch above never
                    // built its session. The peripheral that gets connected to
                    // is always the responder — build that session now so its
                    // KeyOffer-onward writes reach the real machine and the
                    // contact persists (`2026-06-08-ios-ble-responder-persist`).
                    // No-op for the central, which already holds an active
                    // session from discovery.
                    if matches!(&hw_event, vauchi_core::Event::BleConnected { .. })
                        && !engine.ble_handshake_session_active()
                        && matches!(engine.current_app_screen(), AppScreen::BleExchange { .. })
                    {
                        engine.start_ble_handshake_as_responder();
                    }
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
        use crate::domain_command::DomainCommand;

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;

        match command {
            cmd @ (DomainCommand::GrantConsent { .. }
            | DomainCommand::RevokeConsent { .. }
            | DomainCommand::CheckConsent { .. }
            | DomainCommand::GetConsentStatus { .. }
            | DomainCommand::GetConsentRecords
            | DomainCommand::IsContentUpdatesSupported
            | DomainCommand::CheckContentUpdates
            | DomainCommand::ApplyContentUpdates
            | DomainCommand::ReloadSocialNetworks
            | DomainCommand::HasSeenAhaMoment { .. }
            | DomainCommand::TryTriggerAhaMoment { .. }
            | DomainCommand::TryTriggerAhaMomentWithContext { .. }
            | DomainCommand::AhaMomentsSeenCount
            | DomainCommand::AhaMomentsTotalCount
            | DomainCommand::ResetAhaMoments
            | DomainCommand::InitDemoContactIfNeeded
            | DomainCommand::GetDemoContact
            | DomainCommand::GetDemoContactState
            | DomainCommand::IsDemoUpdateAvailable
            | DomainCommand::TriggerDemoUpdate
            | DomainCommand::DismissDemoContact
            | DomainCommand::AutoRemoveDemoContact
            | DomainCommand::RestoreDemoContact) => self.dispatch_engagement(&mut engine, cmd),
            cmd @ (DomainCommand::GetOwnCard
            | DomainCommand::AddField { .. }
            | DomainCommand::UpdateField { .. }
            | DomainCommand::RemoveField { .. }
            | DomainCommand::SetDisplayName { .. }
            | DomainCommand::SetOwnAvatar { .. }
            | DomainCommand::ClearOwnAvatar
            | DomainCommand::CreateIdentity { .. }
            | DomainCommand::GetPublicId
            | DomainCommand::GetDisplayName
            | DomainCommand::GetOwnFingerprint
            | DomainCommand::DisplayNameSuggestions { .. }
            | DomainCommand::ResetOnboarding
            | DomainCommand::GetOnboardingProgress
            | DomainCommand::CurrentOnboardingStep
            | DomainCommand::IsOnboardingComplete
            | DomainCommand::AdvanceOnboarding
            | DomainCommand::SkipOnboardingStep) => {
                self.dispatch_own_card_identity(&mut engine, cmd)
            }
            cmd @ (DomainCommand::ListContacts
            | DomainCommand::GetContact { .. }
            | DomainCommand::SearchContacts { .. }
            | DomainCommand::ContactCount
            | DomainCommand::RemoveContact { .. }
            | DomainCommand::SoftDeleteImportedContact { .. }
            | DomainCommand::UndoDeleteImportedContact { .. }
            | DomainCommand::HardDeleteImportedContact { .. }
            | DomainCommand::ArchiveContact { .. }
            | DomainCommand::UnarchiveContact { .. }
            | DomainCommand::ListArchivedContacts
            | DomainCommand::HideContact { .. }
            | DomainCommand::UnhideContact { .. }
            | DomainCommand::VerifyContact { .. }
            | DomainCommand::SetProposalTrusted { .. }
            | DomainCommand::FindDuplicates
            | DomainCommand::DismissDuplicate { .. }
            | DomainCommand::SetContactNote { .. }
            | DomainCommand::GetContactNote { .. }
            | DomainCommand::DeleteContactNote { .. }
            | DomainCommand::SetContactFieldNote { .. }
            | DomainCommand::GetContactFieldNotes { .. }
            | DomainCommand::DeleteContactFieldNote { .. }
            | DomainCommand::SetContactNickname { .. }
            | DomainCommand::ClearContactNickname { .. }
            | DomainCommand::SetContactCustomAvatar { .. }
            | DomainCommand::ClearContactCustomAvatar { .. }
            | DomainCommand::GetContactCustomAvatar { .. }
            | DomainCommand::SearchSocialNetworks { .. }
            | DomainCommand::GetProfileUrl { .. }
            | DomainCommand::ListHiddenContacts
            | DomainCommand::ContactDetailFooterActionId { .. }
            | DomainCommand::SetDisplayNamePreference { .. }
            | DomainCommand::SetAvatarPreference { .. }
            | DomainCommand::MergeContacts { .. }
            | DomainCommand::GetContactDisplayOptions { .. }
            | DomainCommand::ListContactsPaginated { .. }
            | DomainCommand::ContactDetailViewState { .. }
            | DomainCommand::ListSocialNetworks) => self.dispatch_contacts(&mut engine, cmd),
            cmd @ (DomainCommand::ListLabels
            | DomainCommand::CreateLabel { .. }
            | DomainCommand::GetLabel { .. }
            | DomainCommand::RenameLabel { .. }
            | DomainCommand::DeleteLabel { .. }
            | DomainCommand::AddContactToGroup { .. }
            | DomainCommand::RemoveContactFromGroup { .. }
            | DomainCommand::GetGroupsForContact { .. }
            | DomainCommand::SetGroupFieldVisibility { .. }
            | DomainCommand::SetContactFieldOverride { .. }
            | DomainCommand::RemoveContactFieldOverride { .. }
            | DomainCommand::HideFieldFromContact { .. }
            | DomainCommand::ShowFieldToContact { .. }
            | DomainCommand::IsFieldVisibleToContact { .. }
            | DomainCommand::GetSuggestedLabels) => {
                self.dispatch_groups_visibility(&mut engine, cmd)
            }
            cmd @ (DomainCommand::ExportGdprData
            | DomainCommand::ScheduleIdentityDeletion
            | DomainCommand::CancelIdentityDeletion
            | DomainCommand::ExecuteIdentityDeletion
            | DomainCommand::GetDeletionState
            | DomainCommand::ShredStatus
            | DomainCommand::SoftShred
            | DomainCommand::CancelShred { .. }
            | DomainCommand::HardShred { .. }
            | DomainCommand::PanicShred
            | DomainCommand::SetupAppPassword { .. }
            | DomainCommand::SetupDuressPassword { .. }
            | DomainCommand::Authenticate { .. }
            | DomainCommand::IsPasswordEnabled
            | DomainCommand::IsDuressEnabled
            | DomainCommand::DisableDuress
            | DomainCommand::ConfigureDuressAlerts { .. }
            | DomainCommand::GetDuressSettings
            | DomainCommand::AddDecoyContact { .. }
            | DomainCommand::ListDecoyContacts
            | DomainCommand::DeleteDecoyContact { .. }
            | DomainCommand::SetPinnedCertificate { .. }
            | DomainCommand::IsCertificatePinningEnabled
            | DomainCommand::ConfigureEmergencyBroadcast { .. }
            | DomainCommand::SendEmergencyBroadcast
            | DomainCommand::GetEmergencyConfig
            | DomainCommand::DisableEmergencyBroadcast) => self.dispatch_security(&mut engine, cmd),
            cmd @ (DomainCommand::VerifyRecoveryProof { .. }
            | DomainCommand::UploadGuardianEntries
            | DomainCommand::SaveRecoveryResponse { .. }
            | DomainCommand::TrustContactForRecovery { .. }
            | DomainCommand::UntrustContactForRecovery { .. }
            | DomainCommand::TrustedContactCount
            | DomainCommand::ExportBackup { .. }
            | DomainCommand::ImportBackup { .. }
            | DomainCommand::ExportFullBackup { .. }
            | DomainCommand::ImportFullBackup { .. }
            | DomainCommand::ImportContactsFromVcf { .. }
            | DomainCommand::ParseRecoveryClaim { .. }
            | DomainCommand::GetRecoveryProof
            | DomainCommand::GetRecoveryStatus
            | DomainCommand::CreateRecoveryVoucher { .. }
            | DomainCommand::AddRecoveryVoucher { .. }
            | DomainCommand::CreateRecoveryClaim { .. }) => {
                self.dispatch_recovery_backup(&mut engine, cmd)
            }
            cmd @ (DomainCommand::Sync
            | DomainCommand::PendingUpdateCount
            | DomainCommand::GetDeliveryRecord { .. }
            | DomainCommand::GetAllDeliveryRecords
            | DomainCommand::GetDeliveryRecordsForContact { .. }
            | DomainCommand::CountFailedDeliveries
            | DomainCommand::GetFailedDeliveryRecords
            | DomainCommand::ManualRetry { .. }
            | DomainCommand::GetPendingDeliveries
            | DomainCommand::GetDeliveryCountByStatus { .. }
            | DomainCommand::GetDueRetries
            | DomainCommand::GetRetriesForContact { .. }
            | DomainCommand::GetRetryCount
            | DomainCommand::DeleteRetry { .. }
            | DomainCommand::CalculateRetryBackoff { .. }
            | DomainCommand::GetTotalPendingCount
            | DomainCommand::IsOfflineQueueFull
            | DomainCommand::GetOfflineQueueCapacity
            | DomainCommand::ClearPendingUpdatesForContact { .. }
            | DomainCommand::GetDeliverySummary { .. }
            | DomainCommand::GetDeviceDeliveries { .. }
            | DomainCommand::GetPendingDeviceDeliveries) => {
                self.dispatch_delivery(&mut engine, cmd)
            }
            cmd @ (DomainCommand::IsPrimaryDevice
            | DomainCommand::GetDeviceCount
            | DomainCommand::GetDevices
            | DomainCommand::UnlinkDevice { .. }
            | DomainCommand::GenerateDeviceLinkQr
            | DomainCommand::ParseDeviceLinkQr { .. }
            | DomainCommand::EncodeMultipartQr { .. }) => self.dispatch_devices(&mut engine, cmd),
        }
    }
}
