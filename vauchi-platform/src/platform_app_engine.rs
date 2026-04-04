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

use vauchi_app::ui::{AppEngine, WorkflowEngine};
use vauchi_core::api::{HandlerId, Vauchi, VauchiConfig, VauchiEvent};
use vauchi_core::crypto::SymmetricKey;

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
///     relayUrl: "wss://relay.vauchi.app",
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
    engine: Mutex<AppEngine>,
    /// Active event listener handler ID, used to unregister on replacement.
    event_handler_id: Mutex<Option<HandlerId>>,
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

        std::fs::create_dir_all(&data_path)
            .map_err(|e| MobileError::StorageError(e.to_string()))?;

        let storage_path = data_path.join("vauchi.db");

        let key_array: [u8; 32] = storage_key_bytes.try_into().map_err(|_| {
            MobileError::StorageError("Storage key must be exactly 32 bytes".to_string())
        })?;
        let storage_key = SymmetricKey::try_from_bytes(key_array).map_err(|_| {
            MobileError::StorageError("Degenerate storage key rejected".to_string())
        })?;

        let config = VauchiConfig::with_storage_path(&storage_path)
            .with_relay_url(&relay_url)
            .with_storage_key(storage_key);

        let vauchi = Vauchi::new(config).map_err(|e| MobileError::Internal(e.to_string()))?;

        Ok(Arc::new(Self {
            engine: Mutex::new(AppEngine::new(vauchi)),
            event_handler_id: Mutex::new(None),
        }))
    }

    /// Returns the current screen as a JSON string.
    ///
    /// The JSON structure matches `ScreenModel` from vauchi-core.
    pub fn current_screen_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        screen_to_json(&engine.current_screen())
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
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let result = engine.handle_action(action);
        action_result_to_json(&result)
    }

    /// Navigate to a screen (as JSON) and return the new screen model as JSON.
    ///
    /// The screen JSON must match the `AppScreen` enum format, e.g.:
    /// - `"Exchange"` (simple variant)
    /// - `{"ContactDetail": {"contact_id": "abc"}}` (parameterized variant)
    pub fn navigate_to_json(&self, screen_json: String) -> Result<String, MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.navigate_to(screen);
        screen_to_json(&model)
    }

    /// Navigate back in the history stack.
    ///
    /// Returns the previous screen model as JSON.
    pub fn navigate_back_json(&self) -> Result<String, MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.navigate_back();
        screen_to_json(&model)
    }

    /// Returns the available navigation screens as a JSON array.
    ///
    /// These are the screens that should appear in the navigation bar/tabs.
    pub fn available_screens_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let screens = engine.available_screens();
        serde_json::to_string(&screens)
            .map_err(|e| MobileError::Internal(format!("Failed to serialize screens: {e}")))
    }

    /// Returns the default landing screen as a JSON string.
    ///
    /// Returns MyInfo when no contacts, Contacts when >=1 contact.
    pub fn default_screen_json(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let screen = engine.default_screen();
        serde_json::to_string(&screen)
            .map_err(|e| MobileError::Internal(format!("Failed to serialize screen: {e}")))
    }

    /// Returns the current screen's screen_id (lightweight query).
    ///
    /// Useful for tab bar highlighting without deserializing the full ScreenModel.
    pub fn current_screen_id(&self) -> Result<String, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        let model = engine.current_screen();
        Ok(model.screen_id)
    }

    /// Invalidate all cached engines.
    ///
    /// Call this after mutations via `VauchiPlatform` so the next
    /// `current_screen_json()` rebuilds engines with fresh data.
    pub fn invalidate_all(&self) -> Result<(), MobileError> {
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.invalidate_all();
        Ok(())
    }

    /// Invalidate a specific screen's cached engine.
    ///
    /// The screen JSON must match the `AppScreen` enum format.
    pub fn invalidate_screen_json(&self, screen_json: String) -> Result<(), MobileError> {
        let screen = app_screen_from_json(&screen_json)?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        engine.invalidate_screen(&screen);
        Ok(())
    }

    /// Returns whether the user has created an identity.
    ///
    /// Used by frontends to decide between onboarding and main UI.
    pub fn has_identity(&self) -> Result<bool, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        Ok(engine.has_identity())
    }

    /// Returns whether the current form has unsaved user data.
    ///
    /// Used by frontends to show a "discard changes?" prompt on back navigation.
    pub fn form_has_data(&self) -> Result<bool, MobileError> {
        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        Ok(engine.form_has_data())
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
            serde_json::from_str(&capabilities_json)
                .map_err(|e| MobileError::Internal(format!("Invalid capabilities JSON: {e}")))?;
        let mut engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
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

        let engine = self
            .engine
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;

        // Remove previous handler if any
        let mut handler_id_slot = self
            .event_handler_id
            .lock()
            .map_err(|e| MobileError::Internal(format!("Lock failed: {e}")))?;
        if let Some(old_id) = handler_id_slot.take() {
            engine.vauchi().remove_event_handler(old_id);
        }

        // Register a new handler that maps VauchiEvents to screen IDs
        let listener_clone = Arc::clone(&listener);
        let new_id = engine
            .vauchi()
            .add_event_handler(Arc::new(move |event: VauchiEvent| {
                let screen_ids = affected_screens(&event);
                if !screen_ids.is_empty() {
                    listener_clone.on_screens_invalidated(screen_ids);
                }
            }));

        *handler_id_slot = Some(new_id);
        Ok(())
    }
}

/// Map a `VauchiEvent` to the screen IDs that would be affected.
fn affected_screens(event: &VauchiEvent) -> Vec<String> {
    match event {
        VauchiEvent::ContactAdded { .. }
        | VauchiEvent::ContactUpdated { .. }
        | VauchiEvent::ContactRemoved { .. }
        | VauchiEvent::ContactHidden { .. }
        | VauchiEvent::ContactUnhidden { .. }
        | VauchiEvent::ContactBlocked { .. }
        | VauchiEvent::ContactUnblocked { .. }
        | VauchiEvent::ContactSoftDeleted { .. }
        | VauchiEvent::ContactArchived { .. }
        | VauchiEvent::ContactUnarchived { .. } => {
            vec!["contacts".into(), "contact_detail".into()]
        }
        VauchiEvent::OwnCardUpdated { .. } => vec!["my_info".into()],
        VauchiEvent::SyncStateChanged { .. }
        | VauchiEvent::SyncProgress { .. }
        | VauchiEvent::LabelSyncCompleted { .. } => {
            vec!["sync".into(), "contacts".into()]
        }
        VauchiEvent::MessageDelivered { .. }
        | VauchiEvent::MessageFailed { .. }
        | VauchiEvent::DeliveryStatusUpdate { .. }
        | VauchiEvent::PreExpiryWarning { .. } => {
            vec!["delivery_status".into()]
        }
        VauchiEvent::ConnectionStateChanged { .. }
        | VauchiEvent::RelayHealthChanged { .. }
        | VauchiEvent::RelayFailover { .. } => {
            vec!["sync".into()]
        }
        VauchiEvent::IncomingUpdate { .. } => {
            vec!["contacts".into(), "contact_detail".into()]
        }
        VauchiEvent::VisibilityChanged { .. } => {
            vec!["my_info".into(), "contacts".into()]
        }
        VauchiEvent::EmergencyAlertReceived { .. } | VauchiEvent::EmergencyBroadcastSent { .. } => {
            vec!["contacts".into()]
        }
        VauchiEvent::DowngradeDetected { .. } | VauchiEvent::Error { .. } => vec![],
        // VauchiEvent is #[non_exhaustive] — unknown future variants
        // don't invalidate any specific screen.
        _ => vec![],
    }
}
