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

use crate::mobile_exchange::serialize_exchange_payload;
use crate::multistage_exchange::{
    MobileMultiStageSession, MobileProtocolState, MobileQrPayload, MultiStageSessionListener,
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
    action_result_to_json, app_screen_from_json, screen_to_json, user_action_from_json,
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
type DirectListenerSlot = Arc<Mutex<Option<Arc<Box<dyn PlatformEventListener>>>>>;

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
    /// Pair 4 — auto-managed `MobileMultiStageSession` for the
    /// `MultiStageExchange` screen. Created by core when navigation
    /// enters the screen, cancelled when navigation leaves. Frontends
    /// never see this — they only fire `UserAction`s and
    /// `ExchangeHardwareEvent`s and render the resulting `ScreenModel`.
    multi_stage_session: Mutex<Option<Arc<MobileMultiStageSession>>>,
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
        let on_multi_stage = matches!(pre_screen, AppScreen::MultiStageExchange);
        let is_retry = matches!(
            &action,
            vauchi_app::ui::UserAction::ActionPressed { action_id }
                if action_id == vauchi_app::ui::MULTI_STAGE_RETRY_ACTION_ID
        );
        if on_multi_stage && is_retry {
            self.cancel_multi_stage_session();
            self.ensure_multi_stage_session()?;
        }

        let result = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.handle_action(action)
        };
        self.after_screen_transition(pre_screen)?;
        action_result_to_json(&result)
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
        event: crate::MobileExchangeHardwareEvent,
    ) -> Result<Option<String>, MobileError> {
        let hw_event: vauchi_core::exchange::ExchangeHardwareEvent = event.into();
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
            AppScreen::MultiStageExchange
        );
        if on_multi_stage
            && let vauchi_core::exchange::ExchangeHardwareEvent::QrScanned { data } = &hw_event
        {
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
        let model = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.navigate_to(screen)
        };
        self.after_screen_transition(pre_screen)?;
        screen_to_json(&model)
    }

    /// Navigate back in the history stack.
    ///
    /// Returns the previous screen model as JSON.
    pub fn navigate_back_json(&self) -> Result<String, MobileError> {
        let pre_screen = self
            .engine
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        let model = {
            let mut engine = self.engine.lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            engine.navigate_back()
        };
        self.after_screen_transition(pre_screen)?;
        screen_to_json(&model)
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
}

impl PlatformAppEngine {
    /// Detect transitions in/out of `MultiStageExchange` and manage the
    /// session lifecycle accordingly. Called after every operation
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
        let was_multi = matches!(pre, AppScreen::MultiStageExchange);
        let is_multi = matches!(post, AppScreen::MultiStageExchange);
        match (was_multi, is_multi) {
            (true, false) => self.cancel_multi_stage_session(),
            (false, true) => self.ensure_multi_stage_session()?,
            _ => {}
        }
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

    // ── Test-only helpers ──────────────────────────────────────────
    //
    // These bridge entry points exist so integration tests can drive
    // the multi-stage screen state without spinning up a real cycle
    // thread + peer. They are not part of the UniFFI surface — Swift
    // / Kotlin frontends never see them, and the production bridge
    // (`MultiStageEngineBridge`) goes straight to
    // `AppEngine::apply_multi_stage_*`. The `_for_test` suffix mirrors
    // the existing convention (`set_cycle_sleep_override_ms_for_test`).

    /// Test-only: drive the active engine's protocol state directly.
    #[doc(hidden)]
    pub fn apply_multi_stage_state_for_test(
        &self,
        state: MobileProtocolState,
    ) -> Result<(), MobileError> {
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
