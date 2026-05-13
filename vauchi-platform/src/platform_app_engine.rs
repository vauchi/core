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

use crate::MobileDeviceLinkSession;
use crate::mobile_exchange::serialize_exchange_payload;
use crate::multistage_exchange::{
    MobileAudioProximityState, MobileMultiStageSession, MobileProtocolState, MobileQrPayload,
    MultiStageAudioListener, MultiStageSessionListener,
};
use crate::types::{
    MobileLocale, MobileNotificationCategory, MobilePendingNotification, MobileTabInfo,
    MobileTabLayout,
};
use vauchi_app::notification_types::NotificationCategory as CoreNotificationCategory;
use vauchi_app::ui::{AppEngine, AppScreen, WorkflowEngine};
use vauchi_core::api::{HandlerId, Vauchi, VauchiConfig, VauchiEvent};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::ProtocolState;

use crate::error::MobileError;
use crate::json_helpers::{
    action_result_envelope_to_json, action_result_to_json, app_screen_from_json,
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
/// Persisted sync-flag state (B7 batch 18). Stored as JSON next
/// to the storage directory; mirrors the legacy `VauchiPlatform`
/// in-memory mutex slots but survives restarts.
struct SyncFlags {
    delivery_receipts_enabled: bool,
    suppress_presence: bool,
}

impl Default for SyncFlags {
    fn default() -> Self {
        Self {
            delivery_receipts_enabled: true,
            suppress_presence: false,
        }
    }
}

impl SyncFlags {
    fn to_json(&self) -> String {
        format!(
            "{{\"delivery_receipts_enabled\":{},\"suppress_presence\":{}}}",
            self.delivery_receipts_enabled, self.suppress_presence
        )
    }

    fn from_json(s: &str) -> Option<Self> {
        let v: serde_json::Value = serde_json::from_str(s).ok()?;
        Some(Self {
            delivery_receipts_enabled: v
                .get("delivery_receipts_enabled")
                .and_then(|x| x.as_bool())
                .unwrap_or(true),
            suppress_presence: v
                .get("suppress_presence")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        })
    }
}

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
    /// Pair 4 — auto-managed `MobileMultiStageSession` for the
    /// `MultiStageExchange` screen. Created by core when navigation
    /// enters the screen, cancelled when navigation leaves. Frontends
    /// never see this — they only fire `UserAction`s and
    /// `Event`s and render the resulting `ScreenModel`.
    multi_stage_session: Mutex<Option<Arc<MobileMultiStageSession>>>,
    /// Pair 5 — auto-managed `MobileDeviceLinkSession` for the
    /// `DeviceLinking` screen. Same lifecycle pattern as multi-stage:
    /// created on navigation in, cancelled on navigation out. The
    /// `DeviceLinkEngineBridge` listener pushes cycle-thread events
    /// (`on_qr_ready`, `on_confirmation_required`, `on_completed`,
    /// `on_failed`) into `AppEngine`'s receiver-side bridge methods.
    device_link_session: Mutex<Option<Arc<MobileDeviceLinkSession>>>,
    /// Phase 1.6 — auto-managed `MobileLinkResponderSession` for the
    /// `DeepLinkResponder` screen. Created lazily on the first
    /// `current_link_responder_session()` call after navigation enters
    /// the screen, cancelled when navigation leaves. Frontends fetch
    /// the cached session via the public getter rather than building
    /// one themselves — the deep-link `DeepLinkPayload` is private to
    /// the cached `AppScreen`, and the local card payload is built
    /// from the engine's own `Vauchi`.
    link_responder_session: Mutex<Option<Arc<crate::MobileLinkResponderSession>>>,
    /// Storage path retained for in-place session creation.
    /// Mirrors `VauchiPlatform::storage_path` so the engine can build
    /// `MobileMultiStageSession::with_persistence` instances without
    /// depending on a sibling `VauchiPlatform` instance.
    storage_path: PathBuf,
    /// Storage key retained for in-place session creation. See
    /// `storage_path` above.
    storage_key: SymmetricKey,
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
            multi_stage_session: Mutex::new(None),
            device_link_session: Mutex::new(None),
            link_responder_session: Mutex::new(None),
            storage_path,
            storage_key,
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
            self.cancel_multi_stage_session();
            self.ensure_multi_stage_session()?;
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
            let session_clone = self
                .multi_stage_session
                .lock()
                .map_err(|e| MobileError::Other {
                    detail: format!("Lock failed: {e}"),
                })?
                .clone();
            if let Some(session) = session_clone {
                let _ = session.process_scanned_qr(value.clone());
            }
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
        // Pair 5 — translate device-link typed ActionResults into
        // `MobileDeviceLinkSession` calls. The engine has already
        // advanced its step (e.g. Completing for ConfirmManual,
        // QrPending for Retry); the session push then drives the next
        // state via the bridge.
        self.dispatch_device_link_side_effects(&result)?;
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
    pub fn handle_hardware_event(
        &self,
        event: crate::MobileEvent,
    ) -> Result<Option<String>, MobileError> {
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
        if on_multi_stage && let vauchi_core::Event::QrScanned { data } = &hw_event {
            let session_clone = self
                .multi_stage_session
                .lock()
                .map_err(|e| MobileError::Other {
                    detail: format!("Lock failed: {e}"),
                })?
                .clone();
            if let Some(session) = session_clone {
                let _ = session.process_scanned_qr(data.clone());
                // The bridge listener will push state changes via the
                // cycle thread; no immediate ActionResult is needed
                // because rendering re-fetches via current_screen_json
                // after the listener fires `on_screens_invalidated`.
                return Ok(None);
            }
            // No session active despite being on the screen — return
            // None and let the engine's default fall-through render.
        }

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        match engine.handle_hardware_event(hw_event) {
            Some(result) => Ok(Some(action_result_to_json(&result)?)),
            None => Ok(None),
        }
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

    /// Navigate to a screen (as JSON) and return the new screen model as JSON.
    ///
    /// The screen JSON must match the `AppScreen` enum format, e.g.:
    /// - `"Exchange"` (simple variant)
    /// - `{"ContactDetail": {"contact_id": "abc"}}` (parameterized variant)
    pub fn navigate_to_json(&self, screen_json: String) -> Result<String, MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
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
            let model = engine.navigate_to(screen);
            let cmds = engine.drain_pending_commands();
            (model, cmds)
        };
        self.after_screen_transition(pre_screen)?;
        screen_envelope_to_json(&model, &pending_commands)
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

    /// Returns the available navigation screens as a JSON array.
    ///
    /// These are the screens that should appear in the navigation bar/tabs.
    pub fn available_screens_json(&self) -> Result<String, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let screens = engine.available_screens();
        serde_json::to_string(&screens).map_err(|e| MobileError::Other {
            detail: format!("Failed to serialize screens: {e}"),
        })
    }

    /// Returns the default landing screen as a JSON string.
    ///
    /// Returns MyInfo when no contacts, Contacts when >=1 contact.
    pub fn default_screen_json(&self) -> Result<String, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let screen = engine.default_screen();
        serde_json::to_string(&screen).map_err(|e| MobileError::Other {
            detail: format!("Failed to serialize screen: {e}"),
        })
    }

    /// Returns the current screen's screen_id (lightweight query).
    ///
    /// Useful for tab bar highlighting without deserializing the full ScreenModel.
    pub fn current_screen_id(&self) -> Result<String, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        self_heal_post_auth(&mut engine);
        let model = engine.current_screen();
        Ok(model.screen_id)
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

    /// Decide what to do after a successful platform biometric
    /// authentication, in constant wall-clock time.
    ///
    /// Frontends call this immediately after the OS biometric prompt
    /// (iOS `LAContext`, Android `BiometricPrompt`) resolves with
    /// success. The call returns either:
    ///
    /// - [`MobileBiometricUnlockOutcome::Unlocked`] — biometric
    ///   proves the real user; the frontend can transition to the
    ///   post-auth screen. `auth_mode` is set to `Normal` in core.
    /// - [`MobileBiometricUnlockOutcome::PromptForDuressPin`] —
    ///   duress is configured; the frontend must show the PIN entry
    ///   screen. The subsequent `authenticate(pin)` call decides
    ///   `Normal` vs `Duress`.
    ///
    /// The call always takes at least
    /// [`vauchi_core::api::vauchi::BIOMETRIC_UNLOCK_MIN_DURATION`]
    /// (300 ms). Padding lives in core so iOS / Android cannot
    /// diverge on the constant-time floor that hides whether duress
    /// is configured (audit item P2-B,
    /// `2026-04-28-lifecycle-session-residue-umbrella`).
    pub fn biometric_unlock_check(
        &self,
    ) -> Result<crate::types::MobileBiometricUnlockOutcome, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let outcome =
            engine
                .vauchi_mut()
                .biometric_unlock_check()
                .map_err(|e| MobileError::Other {
                    detail: e.to_string(),
                })?;
        Ok(outcome.into())
    }

    /// Returns whether the current form has unsaved user data.
    ///
    /// Used by frontends to show a "discard changes?" prompt on back navigation.
    pub fn form_has_data(&self) -> Result<bool, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine.form_has_data())
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
    /// Companion constants on the core side
    /// (`PERIODIC_SYNC_INTERVAL_SECONDS = 900`,
    /// `PERIODIC_SYNC_MAX_RETRIES = 3`) replace the duplicated
    /// 15-min interval / 3-retry magic numbers in
    /// `BackgroundSyncService` / `SyncWorker`.
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

    /// Recommended interval (seconds) between periodic sync ticks.
    ///
    /// Frontends call this once at scheduler-registration time to
    /// configure their `BGTaskScheduler` / `WorkManager` interval.
    /// Single source of truth lives in core
    /// ([`vauchi_core::PERIODIC_SYNC_INTERVAL_SECONDS`]).
    pub fn periodic_sync_interval_seconds(&self) -> u64 {
        vauchi_core::PERIODIC_SYNC_INTERVAL_SECONDS
    }

    /// Maximum retries the platform scheduler should configure for
    /// a failed periodic sync. Single source of truth lives in core
    /// ([`vauchi_core::PERIODIC_SYNC_MAX_RETRIES`]).
    pub fn periodic_sync_max_retries(&self) -> u32 {
        vauchi_core::PERIODIC_SYNC_MAX_RETRIES
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

    /// Returns the last frontend-reported network reachability.
    ///
    /// Defaults to `true` until the frontend reports otherwise. Used
    /// by reachability tests; frontends do not need to query this —
    /// the offline banner is injected into emitted `ScreenModel`s
    /// automatically when the state is `false`.
    pub fn is_network_online(&self) -> Result<bool, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        Ok(engine.is_network_online())
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

        // Remove previous handler if any
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

    /// Drain pending OS notifications.
    ///
    /// Returns notifications that should be shown to the user via the
    /// platform's native notification system. Each call clears the buffer,
    /// so notifications are never returned twice.
    ///
    /// Call this after receiving `on_screens_invalidated` from your
    /// `PlatformEventListener`.
    pub fn drain_pending_notifications(
        &self,
    ) -> Result<Vec<MobilePendingNotification>, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let notifications = engine.drain_pending_notifications();
        Ok(notifications
            .into_iter()
            .map(|n| {
                use vauchi_app::notification_types::NotificationCategory;
                MobilePendingNotification {
                    event_key: n.event_key,
                    category: match n.category {
                        NotificationCategory::EmergencyAlert => {
                            MobileNotificationCategory::EmergencyAlert
                        }
                        NotificationCategory::ContactAdded => {
                            MobileNotificationCategory::ContactAdded
                        }
                    },
                    title: n.title,
                    body: n.body,
                    contact_id: n.contact_id,
                }
            })
            .collect())
    }

    // ── Recovery (Phase B2 — collapse-vauchi-platform-into-app-engine) ──
    //
    // Wraps the recovery domain that previously only lived on
    // `VauchiPlatform`. Frontends migrating in Phase C1 / C7 stop
    // touching the legacy struct and route every recovery operation
    // through the engine. Cache invalidation targets the `Recovery`
    // and `RecoveryHelp` screens so reads after a write reflect the
    // mutation without an explicit `invalidate_*` call from the caller.

    /// Create a recovery claim binding `old_pk_hex` (lost identity) to
    /// the active identity's signing public key.
    pub fn create_recovery_claim(
        &self,
        old_pk_hex: String,
    ) -> Result<crate::types::MobileRecoveryClaim, MobileError> {
        use base64::Engine as _;
        use vauchi_core::recovery::{RecoveryClaim, RecoveryProof};

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let identity = engine
            .vauchi()
            .identity()
            .ok_or_else(|| MobileError::Other {
                detail: "Identity not initialized".into(),
            })?;

        let old_pk_bytes = hex::decode(&old_pk_hex).map_err(|e| MobileError::InvalidInput {
            field: String::new(),
            detail: format!("Invalid hex: {e}"),
        })?;
        let old_pk: [u8; 32] = old_pk_bytes
            .try_into()
            .map_err(|_| MobileError::InvalidInput {
                field: String::new(),
                detail: "Public key must be 32 bytes".into(),
            })?;

        let new_pk = *identity.signing_public_key();
        let claim = RecoveryClaim::new(&old_pk, &new_pk);

        // Persist a `RecoveryProof` shell beside the database — this
        // mirrors the legacy `VauchiPlatform` file layout so the two
        // surfaces share state during the Phase-C migration window.
        // Threshold matches the legacy default (3).
        let proof = RecoveryProof::new(&old_pk, &new_pk, 3);
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
            is_expired: claim.is_expired(),
        };

        engine.invalidate_screen(&AppScreen::Recovery);
        engine.invalidate_screen(&AppScreen::RecoveryHelp);
        Ok(result)
    }

    /// Parse a base64-encoded recovery claim. Read-only — does not
    /// touch the recovery proof file.
    pub fn parse_recovery_claim(
        &self,
        claim_b64: String,
    ) -> Result<crate::types::MobileRecoveryClaim, MobileError> {
        use base64::Engine as _;
        use vauchi_core::recovery::RecoveryClaim;

        let claim_bytes = base64::engine::general_purpose::STANDARD
            .decode(&claim_b64)
            .map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid base64: {e}"),
            })?;
        let claim =
            RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid claim: {e}"),
            })?;

        Ok(crate::types::MobileRecoveryClaim {
            old_public_key: hex::encode(claim.old_pk()),
            new_public_key: hex::encode(claim.new_pk()),
            claim_data: claim_b64,
            is_expired: claim.is_expired(),
        })
    }

    /// Create a voucher for someone else's recovery claim using the
    /// active identity's signing key.
    pub fn create_recovery_voucher(
        &self,
        claim_b64: String,
    ) -> Result<crate::types::MobileRecoveryVoucher, MobileError> {
        use base64::Engine as _;
        use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
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
        let claim =
            RecoveryClaim::from_bytes(&claim_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid claim: {e}"),
            })?;

        if claim.is_expired() {
            return Err(MobileError::InvalidInput {
                field: String::new(),
                detail: "Claim has expired".into(),
            });
        }

        let voucher = RecoveryVoucher::create_from_claim(&claim, identity.signing_keypair(), None)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        let voucher_data = base64::engine::general_purpose::STANDARD.encode(voucher.to_bytes());

        Ok(crate::types::MobileRecoveryVoucher {
            voucher_public_key: hex::encode(voucher.voucher_pk()),
            voucher_data,
        })
    }

    /// Add a voucher to the in-progress recovery proof. Requires that
    /// `create_recovery_claim` was called first.
    pub fn add_recovery_voucher(
        &self,
        voucher_b64: String,
    ) -> Result<crate::types::MobileRecoveryProgress, MobileError> {
        use base64::Engine as _;
        use vauchi_core::recovery::{RecoveryProof, RecoveryVoucher};

        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;

        let voucher_bytes = base64::engine::general_purpose::STANDARD
            .decode(&voucher_b64)
            .map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid base64: {e}"),
            })?;
        let voucher =
            RecoveryVoucher::from_bytes(&voucher_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid voucher: {e}"),
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
        let proof_bytes = std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        let mut proof =
            RecoveryProof::from_bytes(&proof_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid proof: {e}"),
            })?;

        let contacts =
            engine
                .vauchi()
                .storage()
                .list_contacts()
                .map_err(|e| MobileError::StorageError {
                    detail: e.to_string(),
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
        std::fs::write(&proof_path, updated_bytes).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
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
        Ok(progress)
    }

    /// Read the in-progress recovery status, if any.
    pub fn get_recovery_status(
        &self,
    ) -> Result<Option<crate::types::MobileRecoveryProgress>, MobileError> {
        use vauchi_core::recovery::RecoveryProof;

        let proof_path = self.recovery_proof_path();
        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes = std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        let proof =
            RecoveryProof::from_bytes(&proof_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid proof: {e}"),
            })?;

        Ok(Some(crate::types::MobileRecoveryProgress {
            old_public_key: hex::encode(proof.old_pk()),
            new_public_key: hex::encode(proof.new_pk()),
            vouchers_collected: proof.voucher_count() as u32,
            vouchers_needed: proof.threshold(),
            is_complete: proof.voucher_count() >= proof.threshold() as usize,
        }))
    }

    /// Read the completed recovery proof as base64. Returns `None`
    /// until the threshold is met.
    pub fn get_recovery_proof(&self) -> Result<Option<String>, MobileError> {
        use base64::Engine as _;
        use vauchi_core::recovery::RecoveryProof;

        let proof_path = self.recovery_proof_path();
        if !proof_path.exists() {
            return Ok(None);
        }

        let proof_bytes = std::fs::read(&proof_path).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        let proof =
            RecoveryProof::from_bytes(&proof_bytes).map_err(|e| MobileError::InvalidInput {
                field: String::new(),
                detail: format!("Invalid proof: {e}"),
            })?;

        if proof.voucher_count() >= proof.threshold() as usize {
            let bytes = proof.to_bytes().map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
            Ok(Some(
                base64::engine::general_purpose::STANDARD.encode(bytes),
            ))
        } else {
            Ok(None)
        }
    }

    /// Mark a contact as recovery-trusted. Blocked contacts cannot be
    /// trusted for recovery.
    pub fn trust_contact_for_recovery(&self, contact_id: String) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
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
        Ok(())
    }

    /// Remove recovery trust from a contact.
    pub fn untrust_contact_for_recovery(&self, contact_id: String) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
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
        Ok(())
    }

    /// Count the contacts marked as recovery-trusted.
    pub fn trusted_contact_count(&self) -> Result<u32, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let contacts =
            engine
                .vauchi()
                .storage()
                .list_contacts()
                .map_err(|e| MobileError::StorageError {
                    detail: e.to_string(),
                })?;
        Ok(contacts.iter().filter(|c| c.is_recovery_trusted()).count() as u32)
    }

    // ── Emergency Broadcast (Phase B3 — collapse-vauchi-platform-into-app-engine) ──
    //
    // Wraps the four emergency-broadcast methods that previously only
    // lived on `VauchiPlatform`. The engine's `Vauchi` instance has
    // identity loaded at construction time, so unlike the legacy code
    // path no `set_identity` call is needed. Cache invalidation
    // targets `Settings` + `EmergencyShred` so the next read after a
    // configure / disable shows the updated state.

    /// Configure the emergency-broadcast destination set, message,
    /// and location-inclusion flag. `contact_ids.len()` must be ≤ the
    /// core-side `MAX_TRUSTED_CONTACTS` cap (enforced by `Vauchi`).
    pub fn configure_emergency_broadcast(
        &self,
        contact_ids: Vec<String>,
        message: String,
        include_location: bool,
    ) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine
            .vauchi_mut()
            .configure_emergency_broadcast(contact_ids, message, include_location)
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        engine.invalidate_screen(&AppScreen::Settings);
        engine.invalidate_screen(&AppScreen::EmergencyShred);
        Ok(())
    }

    /// Send the configured emergency broadcast. Errors when no
    /// configuration exists (caller must `configure_emergency_broadcast`
    /// first). Returns the count of alerts queued vs total trusted
    /// contacts.
    pub fn send_emergency_broadcast(
        &self,
    ) -> Result<crate::types::MobileBroadcastResult, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let result = engine
            .vauchi_mut()
            .send_emergency_broadcast()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        engine.invalidate_screen(&AppScreen::Settings);
        engine.invalidate_screen(&AppScreen::EmergencyShred);
        Ok(crate::types::MobileBroadcastResult {
            sent: result.sent as u32,
            total: result.total as u32,
        })
    }

    /// Read the current emergency-broadcast configuration. Returns
    /// `None` when never configured (or after `disable_emergency_broadcast`).
    pub fn get_emergency_config(
        &self,
    ) -> Result<Option<crate::types::MobileEmergencyConfig>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let config = engine
            .vauchi()
            .load_emergency_config()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        Ok(config.map(|c| crate::types::MobileEmergencyConfig {
            trusted_contact_ids: c.trusted_contact_ids,
            message: c.message,
            include_location: c.include_location,
        }))
    }

    /// Delete the emergency-broadcast configuration. Idempotent —
    /// calling on a never-configured instance succeeds silently
    /// (matches legacy `VauchiPlatform` semantics).
    pub fn disable_emergency_broadcast(&self) -> Result<(), MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        engine
            .vauchi_mut()
            .delete_emergency_config()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?;
        engine.invalidate_screen(&AppScreen::Settings);
        engine.invalidate_screen(&AppScreen::EmergencyShred);
        Ok(())
    }

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
                let storage = engine.vauchi().storage();
                let mut card = storage
                    .load_own_card()
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?
                    .ok_or(MobileError::Other {
                        detail: "Identity not found".into(),
                    })?;
                card.set_display_name(&name)
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
                    .filter(|v| contact_pks.contains(v.voucher_pk()))
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

            // ── Sync flag persistence (B7 batch 18) ──
            DomainCommand::IsDeliveryReceiptsEnabled => {
                drop(engine);
                Ok(DomainCommandResult::Bool {
                    value: self.load_sync_flags_engine().delivery_receipts_enabled,
                })
            }
            DomainCommand::SetDeliveryReceiptsEnabled { enabled } => {
                drop(engine);
                let mut flags = self.load_sync_flags_engine();
                flags.delivery_receipts_enabled = enabled;
                self.save_sync_flags_engine(&flags)?;
                Ok(DomainCommandResult::Unit)
            }
            DomainCommand::IsSuppressPresenceEnabled => {
                drop(engine);
                Ok(DomainCommandResult::Bool {
                    value: self.load_sync_flags_engine().suppress_presence,
                })
            }
            DomainCommand::SetSuppressPresenceEnabled { enabled } => {
                drop(engine);
                let mut flags = self.load_sync_flags_engine();
                flags.suppress_presence = enabled;
                self.save_sync_flags_engine(&flags)?;
                Ok(DomainCommandResult::Unit)
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
                if let Some(kind) = reciprocity_banner(contact.reciprocity()) {
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
        }
    }

    // ── Device Linking (Phase B4 — collapse-vauchi-platform-into-app-engine) ──
    //
    // Wraps the **post-orchestrator** device-linking surface. The
    // pre-orchestrator legacy methods (`start_device_link`,
    // `start_device_join`, `send_device_link_request`,
    // `listen_for_device_link_request`, `send_device_link_response`)
    // are intentionally NOT migrated — they were superseded by the
    // orchestrator session in
    // `done/2026-04-27-device-link-orchestrator-phase2d-windows`.

    /// List devices linked to the active identity. The first entry
    /// (index 0) is the primary device.
    pub fn get_devices(&self) -> Result<Vec<crate::types::MobileDeviceInfo>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
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
                    return Ok(vec![crate::types::MobileDeviceInfo {
                        device_index: device_info.device_index(),
                        device_name: device_info.device_name().to_string(),
                        is_current: true,
                        is_active: true,
                        public_key_prefix: hex::encode(&device_info.device_id()[..8]),
                        created_at: device_info.created_at(),
                    }]);
                }
            };

        let current_device_id = identity.device_info().device_id();
        Ok(registry
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
            .collect())
    }

    /// Number of devices linked to the active identity. Returns 1 when
    /// no registry exists yet (only the current device).
    pub fn device_count(&self) -> Result<u32, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let storage = engine.vauchi().storage();

        match storage
            .load_device_registry()
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })? {
            Some(r) => Ok(r.device_count() as u32),
            None => Ok(1),
        }
    }

    /// Returns whether the current device is the primary device
    /// (`device_index == 0`).
    pub fn is_primary_device(&self) -> Result<bool, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let identity = engine
            .vauchi()
            .identity()
            .ok_or_else(|| MobileError::Other {
                detail: "Identity not initialized".into(),
            })?;
        Ok(identity.device_info().device_index() == 0)
    }

    /// Revoke the device at `device_index`. Returns `true` when a
    /// device was revoked, `false` when the index is out of range or
    /// no registry exists. Errors when the caller targets the
    /// current device — frontends must use identity deletion instead.
    pub fn unlink_device(&self, device_index: u32) -> Result<bool, MobileError> {
        let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
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
                None => return Ok(false),
            };

        let devices = registry.all_devices();
        if device_index as usize >= devices.len() {
            return Ok(false);
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
                storage
                    .save_device_registry(&registry)
                    .map_err(|e| MobileError::StorageError {
                        detail: e.to_string(),
                    })?;
                true
            }
            Err(_) => false,
        };

        engine.invalidate_screen(&AppScreen::DeviceManagement);
        engine.invalidate_screen(&AppScreen::DeviceLinking);
        Ok(result)
    }

    /// Generate the QR shown to a peer for device linking. Read-only
    /// — does not persist any state. The QR expires after 300 s
    /// (ADR-035).
    pub fn generate_device_link_qr(
        &self,
    ) -> Result<crate::types::MobileDeviceLinkData, MobileError> {
        use vauchi_core::exchange::device_link::DeviceLinkQR;

        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let identity = engine
            .vauchi()
            .identity()
            .ok_or_else(|| MobileError::Other {
                detail: "Identity not initialized".into(),
            })?;

        let qr = DeviceLinkQR::generate(identity);
        Ok(crate::types::MobileDeviceLinkData {
            qr_data: qr.to_data_string(),
            identity_public_key: hex::encode(identity.signing_public_key()),
            timestamp: qr.timestamp(),
            expires_at: qr.expires_at(),
        })
    }

    /// Parse a peer's device-link QR. Read-only — does not
    /// persist any state.
    pub fn parse_device_link_qr(
        &self,
        qr_data: String,
    ) -> Result<crate::types::MobileDeviceLinkInfo, MobileError> {
        use vauchi_core::exchange::device_link::DeviceLinkQR;

        let qr =
            DeviceLinkQR::from_data_string(&qr_data).map_err(|_| MobileError::InvalidInput {
                field: "qr".into(),
                detail: "Invalid QR code".into(),
            })?;

        Ok(crate::types::MobileDeviceLinkInfo {
            identity_public_key: hex::encode(qr.identity_public_key()),
            timestamp: qr.timestamp(),
            is_expired: qr.is_expired(),
        })
    }

    /// Create the orchestrator session for the initiator side of a
    /// device link. The frontend registers a
    /// `DeviceLinkSessionListener`, calls `start()` on the returned
    /// session, and forwards user actions via `confirm_manual` /
    /// `confirm_ultrasonic` / `deny`. The session owns the
    /// relay-poll loop, the QR-expiry deadline, and the
    /// user-confirm gate. Replaces the legacy split between
    /// `start_device_link()`, `listen_for_device_link_request()`,
    /// and `send_device_link_response()`.
    ///
    /// Persistence: the session saves the updated `DeviceRegistry`
    /// after `confirm_link` succeeds, closing a pre-existing gap
    /// where the legacy single-shot path discarded it.
    pub fn create_device_link_session_initiator(
        &self,
    ) -> Result<Arc<crate::MobileDeviceLinkSession>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let identity = engine
            .vauchi()
            .identity()
            .ok_or_else(|| MobileError::Other {
                detail: "Identity not initialized".into(),
            })?;
        let storage = engine.vauchi().storage();

        let registry = storage
            .load_device_registry()
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })?
            .unwrap_or_else(|| identity.initial_device_registry());

        let initiator = identity.create_device_link_initiator(registry);
        let identity_id = hex::encode(identity.signing_public_key());

        // ADR-035: device-link QR expiry is 300 s — align the
        // relay-listen budget so the cycle thread's deadline matches
        // the QR expiry observed by the peer.
        const RELAY_TIMEOUT_SECS: u64 = 300;

        let relay_url = engine.vauchi().config().relay.server_url.clone();
        let connect_timeout_ms = engine.vauchi().config().relay.connect_timeout_ms;
        let transport = engine
            .vauchi()
            .build_relay_transport(relay_url, connect_timeout_ms.max(10_000));

        Ok(Arc::new(
            crate::mobile_device_link_session::MobileDeviceLinkSession::with_persistence_initiator(
                initiator,
                transport,
                identity_id,
                RELAY_TIMEOUT_SECS,
                self.storage_path.clone(),
                self.storage_key.clone(),
            ),
        ))
    }

    /// Fetch the engine-owned link-mode responder session for the
    /// `DeepLinkResponder` screen. Lazily constructs + caches on the
    /// first call after navigation enters the screen; subsequent calls
    /// return the same `Arc`. Returns `None` whenever the engine is on
    /// any other screen.
    ///
    /// Phase 1.6 of `_private/docs/problems/2026-04-27-deep-link-responder-flow`.
    /// The frontend calls this when it observes
    /// `screen_id == "link_responder_waiting"`, attaches its own
    /// `LinkResponderSessionListener`, and calls `start()`. The engine
    /// `cancel()`s + drops the cached session in
    /// `after_screen_transition` whenever navigation leaves the
    /// responder screen, so the same `current_link_responder_session()`
    /// call after navigate-back returns `None` again.
    pub fn current_link_responder_session(
        &self,
    ) -> Result<Option<Arc<crate::MobileLinkResponderSession>>, MobileError> {
        // Lazy-build on demand. `ensure_link_responder_session` is a
        // no-op if the engine is on any other screen, in which case
        // the slot stays empty and we return `None`.
        self.ensure_link_responder_session()?;
        Ok(self
            .link_responder_session
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .clone())
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
    fn sync_flags_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".sync_flags")
    }

    /// File path holding the pinned TLS certificate PEM (B7 batch 21).
    /// Existence of the file = pinning enabled. Empty / missing = disabled.
    fn cert_pin_path_engine(&self) -> std::path::PathBuf {
        self.storage_path
            .parent()
            .unwrap_or(&self.storage_path)
            .join(".cert_pin")
    }

    fn load_sync_flags_engine(&self) -> SyncFlags {
        let path = self.sync_flags_path_engine();
        if let Ok(data) = std::fs::read_to_string(&path)
            && let Some(flags) = SyncFlags::from_json(&data)
        {
            return flags;
        }
        SyncFlags::default()
    }

    fn save_sync_flags_engine(&self, flags: &SyncFlags) -> Result<(), MobileError> {
        let path = self.sync_flags_path_engine();
        std::fs::write(&path, flags.to_json()).map_err(|e| MobileError::StorageError {
            detail: e.to_string(),
        })?;
        Ok(())
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
    fn after_screen_transition(&self, pre: AppScreen) -> Result<(), MobileError> {
        let post = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        let was_multi = matches!(pre, AppScreen::MultiStageExchange { .. });
        let is_multi = matches!(post, AppScreen::MultiStageExchange { .. });
        match (was_multi, is_multi) {
            (true, false) => self.cancel_multi_stage_session(),
            (false, true) => self.ensure_multi_stage_session()?,
            _ => {}
        }
        let was_link = matches!(pre, AppScreen::DeviceLinking);
        let is_link = matches!(post, AppScreen::DeviceLinking);
        match (was_link, is_link) {
            (true, false) => self.cancel_device_link_session(),
            (false, true) => self.ensure_device_link_session()?,
            _ => {}
        }
        let was_responder = matches!(pre, AppScreen::DeepLinkResponder { .. });
        let is_responder = matches!(post, AppScreen::DeepLinkResponder { .. });
        if was_responder && !is_responder {
            self.cancel_link_responder_session();
        }
        // Note: we do NOT eagerly create on enter — frontends fetch the
        // session lazily via `current_link_responder_session()`. Eager
        // creation would force the engine to construct + cache a session
        // even when the front-end has not yet rendered the screen, which
        // wastes work if the user immediately backs out and means the
        // cycle thread `start()` ordering would have to be coordinated
        // here too. Lazy fetch keeps `start()` on the frontend side,
        // after the listener is wired.
        Ok(())
    }

    /// Build the local exchange payload (identity public key + own
    /// card) required by `MobileMultiStageSession::with_persistence`.
    /// Pulls the current identity + card from the cached AppEngine's
    /// `Vauchi`. Returns an error if no identity exists.
    fn build_exchange_payload(&self) -> Result<Vec<u8>, MobileError> {
        let engine = self.engine.lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let vauchi = engine.vauchi();
        let identity = vauchi.identity().ok_or_else(|| MobileError::Other {
            detail: "Cannot start multi-stage exchange without an identity".to_string(),
        })?;
        // Snapshot the identity-derived bits before dropping the lock —
        // `Identity` is not `Clone` (master seed is zeroized on drop).
        let signing_key = *identity.signing_public_key();
        let display_name = identity.display_name().to_string();
        let card = vauchi
            .own_card()
            .map_err(|e| MobileError::Other {
                detail: e.to_string(),
            })?
            .unwrap_or_else(|| ContactCard::new(&display_name));
        Ok(serialize_exchange_payload(&signing_key, &card))
    }

    /// Lazily create + start the `MobileMultiStageSession` and wire the
    /// core-supplied `MultiStageEngineBridge` listener so cycle-thread
    /// callbacks reach the active engine. Idempotent: a no-op when a
    /// session is already running.
    fn ensure_multi_stage_session(&self) -> Result<(), MobileError> {
        let mut slot = self
            .multi_stage_session
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        if slot.is_some() {
            return Ok(());
        }
        let payload = self.build_exchange_payload()?;
        let session = Arc::new(MobileMultiStageSession::with_persistence(
            payload,
            self.storage_path.clone(),
            self.storage_key.clone(),
        ));
        let bridge = MultiStageEngineBridge {
            engine: Arc::clone(&self.engine),
            direct_listener: Arc::clone(&self.direct_listener),
        };
        session.set_listener(Box::new(bridge));
        // Hover-only wire-up: the cycle-thread autonomous trigger
        // (`try_autonomous_audio_trigger` in `multistage_exchange.rs`)
        // gates on the audio-listener slot — empty slot means no
        // state-machine advance into `Listening` and no audio
        // commands surface to the renderer. Glance flows (and every
        // non-multi-stage active engine) leave the slot empty so the
        // trigger stays a silent no-op. Until the Phase 1.E
        // mode-dispatcher in `screens.rs` flips to per-mode
        // constructors, `is_active_engine_multi_stage_hover` always
        // returns `false`, which is the desired Phase 1.C-polish
        // behaviour: no spurious audio chrome until the dispatcher
        // is wired.
        let is_hover = self
            .engine
            .lock()
            .map(|guard| guard.is_active_engine_multi_stage_hover())
            .unwrap_or(false);
        if is_hover {
            let audio_bridge = MultiStageAudioEngineBridge {
                engine: Arc::clone(&self.engine),
                direct_listener: Arc::clone(&self.direct_listener),
                audio_commands: session.pending_audio_commands_handle(),
            };
            session.set_audio_listener(Box::new(audio_bridge));
        }
        session.start();
        *slot = Some(session);
        Ok(())
    }

    /// Cancel + drop the active `MobileMultiStageSession`. Cancellation
    /// is idempotent — calling this without an active session is a
    /// no-op.
    fn cancel_multi_stage_session(&self) {
        let session_to_cancel = self
            .multi_stage_session
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(session) = session_to_cancel {
            session.cancel();
        }
    }

    /// Lazily create the `MobileLinkResponderSession` for the current
    /// `AppScreen::DeepLinkResponder`. Called from
    /// `current_link_responder_session()` on demand — frontends pull the
    /// cached session when they're ready to attach a listener and call
    /// `start()`.
    ///
    /// Returns `Ok(())` if the engine is on the responder screen and the
    /// slot is now populated (or was already). Returns
    /// `Ok(())` as a no-op when the engine is on any other screen — the
    /// public getter then surfaces `None`. Returns `MobileError` only
    /// for genuine construction failures (no identity, lock poisoning,
    /// non-contributory DH, AEAD RNG failure).
    fn ensure_link_responder_session(&self) -> Result<(), MobileError> {
        let mut slot = self
            .link_responder_session
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
        if slot.is_some() {
            return Ok(());
        }
        // Snapshot the active payload under the engine lock, then drop
        // the lock before doing any DH / encryption work — the helper
        // pulls identity + own_card from the engine's `Vauchi` and we
        // do not want to nest those locks.
        let payload = {
            let engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            match engine.current_app_screen() {
                AppScreen::DeepLinkResponder { payload } => payload.clone(),
                _ => return Ok(()),
            }
        };
        let our_card_bytes = self.build_exchange_payload()?;
        let session =
            crate::MobileLinkResponderSession::from_parsed(payload.as_parsed(), our_card_bytes)?;
        *slot = Some(session);
        Ok(())
    }

    /// Cancel + drop the active `MobileLinkResponderSession`. Idempotent
    /// — a no-op when no session is cached. Mirrors
    /// `cancel_multi_stage_session`.
    fn cancel_link_responder_session(&self) {
        let session_to_cancel = self
            .link_responder_session
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(session) = session_to_cancel {
            session.cancel();
        }
    }

    /// Internal accessor: `engine` Mutex. Used by the Pair 5
    /// device-link wiring in `platform_app_engine_device_link.rs`.
    pub(crate) fn engine(&self) -> &Arc<Mutex<AppEngine>> {
        &self.engine
    }

    /// Internal accessor: `direct_listener` slot.
    pub(crate) fn direct_listener(&self) -> &DirectListenerSlot {
        &self.direct_listener
    }

    /// Internal accessor: device-link session slot.
    pub(crate) fn device_link_session(&self) -> &Mutex<Option<Arc<MobileDeviceLinkSession>>> {
        &self.device_link_session
    }

    // ── Test-only helpers ──────────────────────────────────────────
    //
    // These bridge entry points exist so integration tests can drive
    // the multi-stage screen state without spinning up a real cycle
    // thread + peer. They are not part of the UniFFI surface — Swift
    // / Kotlin frontends never see them, and the production bridge
    // (`MultiStageEngineBridge`) goes straight to
    // `AppEngine::apply_multi_stage_*`. The `_for_test` suffix mirrors
    // the existing convention (`set_cycle_sleep_override_ms_for_test`).
    //
    // Each method calls `cancel_multi_stage_session()` first because
    // `navigate_to(MultiStageExchange)` (typically via the test helper
    // `drive_to_multi_stage`) eagerly starts a real cycle thread via
    // `after_screen_transition` → `ensure_multi_stage_session`. Without
    // the cancel, the cycle thread races the test's manual state pushes
    // — observed as ~20% flake on
    // `apply_multi_stage_state_finalized_with_session_ended_renders_success_screen`
    // where the cycle thread re-pushed `Idle` after the test set
    // `Finalized`, leaving `session_ended` true but state Idle so the
    // `Finalized | Complete | RetryReady if session_ended` arm in
    // `MultiStageExchangeEngine::build_screen` missed and the screen
    // fell through to the active "Waiting for peer…" chrome.
    // `cancel_multi_stage_session` is idempotent (no-op when the slot
    // is already None) so the four `_for_test` entries can call it
    // unconditionally.

    /// Test-only: drive the active engine's protocol state directly.
    #[doc(hidden)]
    pub fn apply_multi_stage_state_for_test(
        &self,
        state: MobileProtocolState,
    ) -> Result<(), MobileError> {
        self.cancel_multi_stage_session();
        let core_state = mobile_state_to_core(state);
        let applied = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.apply_multi_stage_state(core_state)
        };
        if applied {
            self.fire_invalidation_for_test();
        }
        Ok(())
    }

    /// Test-only: push a QR payload into the active engine.
    #[doc(hidden)]
    pub fn apply_multi_stage_qr_payload_for_test(
        &self,
        payload: MobileQrPayload,
    ) -> Result<(), MobileError> {
        self.cancel_multi_stage_session();
        let qr = vauchi_core::exchange::QrPayload {
            data: payload.data,
            error_correction: payload.error_correction,
            display_duration_ms: payload.display_duration_ms,
        };
        let applied = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.apply_multi_stage_qr_payload(&qr)
        };
        if applied {
            self.fire_invalidation_for_test();
        }
        Ok(())
    }

    /// Test-only: record the peer display name on Finalized.
    #[doc(hidden)]
    pub fn apply_multi_stage_finalized_for_test(
        &self,
        contact_name: String,
    ) -> Result<(), MobileError> {
        self.cancel_multi_stage_session();
        let applied = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.apply_multi_stage_finalized(contact_name)
        };
        if applied {
            self.fire_invalidation_for_test();
        }
        Ok(())
    }

    /// Test-only: flag the cycle thread as ended.
    #[doc(hidden)]
    pub fn apply_multi_stage_session_ended_for_test(&self) -> Result<(), MobileError> {
        self.cancel_multi_stage_session();
        let applied = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.apply_multi_stage_session_ended()
        };
        if applied {
            self.fire_invalidation_for_test();
        }
        Ok(())
    }

    fn fire_invalidation_for_test(&self) {
        let listener = self
            .direct_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(vec!["multi_stage_exchange".into()]);
        }
    }
}

/// Core-supplied `MultiStageSessionListener` that bridges cycle-thread
/// callbacks into the active `MultiStageExchangeEngine`.
///
/// Holds field clones (not an `Arc<PlatformAppEngine>`) so the cycle
/// thread mutates engine state directly without re-entering the
/// PlatformAppEngine API surface — and the entry methods on
/// `PlatformAppEngine` stay on `&self` instead of `self: Arc<Self>`.
struct MultiStageEngineBridge {
    engine: Arc<Mutex<AppEngine>>,
    direct_listener: DirectListenerSlot,
}

impl MultiStageEngineBridge {
    fn notify(&self) {
        // Clone the listener arc out from under the lock so the
        // callback fires unlocked — a frontend implementation that
        // re-enters Rust on the callback (typical: read
        // current_screen_json) won't deadlock.
        let listener = self
            .direct_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(vec!["multi_stage_exchange".into()]);
        }
    }
}

impl MultiStageSessionListener for MultiStageEngineBridge {
    fn on_qr_payload(&self, payload: MobileQrPayload) {
        // Errors here are non-actionable — the cycle thread is the
        // only caller and re-attempting won't help. Drop silently per
        // logging-rules.md (no PII; cycle is hot-path).
        let qr = vauchi_core::exchange::QrPayload {
            data: payload.data,
            error_correction: payload.error_correction,
            display_duration_ms: payload.display_duration_ms,
        };
        let applied = match self.engine.lock() {
            Ok(mut e) => e.apply_multi_stage_qr_payload(&qr),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_state_changed(&self, state: MobileProtocolState) {
        let core_state = mobile_state_to_core(state);
        let applied = match self.engine.lock() {
            Ok(mut e) => e.apply_multi_stage_state(core_state),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_finalized(&self, contact_name: String) {
        let applied = match self.engine.lock() {
            Ok(mut e) => e.apply_multi_stage_finalized(contact_name),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }

    fn on_session_ended(&self) {
        let applied = match self.engine.lock() {
            Ok(mut e) => e.apply_multi_stage_session_ended(),
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }
}

/// Core-supplied audio-proximity listener that bridges
/// platform-side orchestrator transitions into the active
/// `MultiStageExchangeEngine` view-state.
///
/// Sibling of [`MultiStageEngineBridge`] — Phase 1.C.3d of
/// `_private/docs/planning/todo/2026-05-11-hover-graduation-plan.md`.
/// The orchestrator that *fires* `on_audio_state_changed` (the
/// `ProximityVerifier` invocation + `Command::AudioEmitChallenge` /
/// `AudioListenForResponse` emission) lands in 1.C.3e; this bridge
/// is dormant plumbing until then — the listener is registered but
/// no callbacks fire.
struct MultiStageAudioEngineBridge {
    engine: Arc<Mutex<AppEngine>>,
    direct_listener: DirectListenerSlot,
    /// Arc-shared handle to the session's pending audio commands
    /// queue. Drained from `on_audio_state_changed` after the
    /// engine-side state apply so the AudioEmit/Listen commands
    /// flow into AppEngine's pending stream in the same callback
    /// tick. Phase 1.C.3e-v.
    audio_commands: std::sync::Arc<std::sync::Mutex<Vec<vauchi_core::Command>>>,
}

impl MultiStageAudioEngineBridge {
    fn notify(&self) {
        // Same `clone-arc-out-of-lock` discipline as
        // `MultiStageEngineBridge::notify` so a frontend
        // implementation that re-enters Rust on the callback (typical:
        // read current_screen_json) won't deadlock.
        let listener = self
            .direct_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_screens_invalidated(vec!["multi_stage_exchange".into()]);
        }
    }
}

impl MultiStageAudioListener for MultiStageAudioEngineBridge {
    fn on_audio_state_changed(&self, state: MobileAudioProximityState) {
        let core_state = mobile_audio_to_core(state);
        let applied = match self.engine.lock() {
            Ok(mut e) => {
                let ok = e.apply_multi_stage_audio_proximity(core_state);
                if ok {
                    // Phase 1.C.3e-v: drain the session's audio
                    // commands and forward into AppEngine's pending
                    // stream so the next screen_envelope_to_json
                    // drain surfaces AudioEmitChallenge /
                    // AudioListenForResponse to the frontend.
                    let cmds = self
                        .audio_commands
                        .lock()
                        .map(|mut q| std::mem::take(&mut *q))
                        .unwrap_or_default();
                    if !cmds.is_empty() {
                        e.extend_pending_commands(cmds);
                    }
                }
                ok
            }
            Err(_) => false,
        };
        if applied {
            self.notify();
        }
    }
}

/// Translate [`MobileAudioProximityState`] (uniffi::Enum) to
/// [`vauchi_core::exchange::AudioProximityState`] (the AppEngine's
/// wire type). Mirrors [`mobile_state_to_core`].
fn mobile_audio_to_core(
    state: MobileAudioProximityState,
) -> vauchi_core::exchange::AudioProximityState {
    use vauchi_core::exchange::AudioProximityState;
    match state {
        MobileAudioProximityState::Pending => AudioProximityState::Pending,
        MobileAudioProximityState::Listening => AudioProximityState::Listening,
        MobileAudioProximityState::Confirmed => AudioProximityState::Confirmed,
        MobileAudioProximityState::Failed => AudioProximityState::Failed,
    }
}

/// Translate `MobileProtocolState` (uniffi::Enum) to
/// `vauchi_core::exchange::ProtocolState` (the AppEngine's wire type).
fn mobile_state_to_core(state: MobileProtocolState) -> ProtocolState {
    match state {
        MobileProtocolState::Idle => ProtocolState::Idle,
        MobileProtocolState::Advertising => ProtocolState::Advertising,
        MobileProtocolState::Discovered => ProtocolState::Discovered,
        MobileProtocolState::Transferring {
            chunks_sent,
            chunks_total,
            chunks_received,
            peer_chunks_total,
        } => ProtocolState::Transferring {
            chunks_sent,
            chunks_total,
            chunks_received,
            peer_chunks_total,
        },
        MobileProtocolState::Verifying => ProtocolState::Verifying,
        MobileProtocolState::Confirming => ProtocolState::Confirming,
        MobileProtocolState::Complete => ProtocolState::Complete,
        MobileProtocolState::Finalized => ProtocolState::Finalized,
        MobileProtocolState::Failed { reason } => ProtocolState::Failed(reason),
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
