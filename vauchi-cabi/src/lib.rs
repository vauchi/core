// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI bindings for vauchi-core workflow engines.
//!
//! Consumed by Windows (C#/P/Invoke) and Linux-Qt (C++/QJsonDocument).
//! All data exchange uses JSON strings.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::Zeroizing;

use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
    ProximityConfidence, ProximityError, ProximityVerifier, VerifierChain, VerifierMethod,
};
use vauchi_core::ui::*;
use vauchi_core::ContactCard;

// ── Type-erased engine wrapper ──────────────────────────────────────

trait WorkflowEngineAny: Send {
    fn current_screen_json(&self) -> String;
    fn handle_action_json(&mut self, json: &str) -> String;
}

impl<T: WorkflowEngine + Send> WorkflowEngineAny for T {
    fn current_screen_json(&self) -> String {
        match serde_json::to_string(&self.current_screen()) {
            Ok(json) => json,
            Err(e) => format!(r#"{{"error":"serialization failed: {}"}}"#, e),
        }
    }

    fn handle_action_json(&mut self, json: &str) -> String {
        match serde_json::from_str::<UserAction>(json) {
            Ok(action) => {
                let result = self.handle_action(action);
                serde_json::to_string(&result).unwrap_or_default()
            }
            Err(e) => format!(r#"{{"error":"{}"}}"#, e),
        }
    }
}

/// Opaque handle to a workflow engine instance.
pub struct VauchiWorkflow {
    engine: Mutex<Box<dyn WorkflowEngineAny>>,
}

// ── String helpers ──────────────────────────────────────────────────

/// Free a string allocated by vauchi-cabi.
///
/// # Safety
/// `ptr` must be a pointer returned by a vauchi_* function, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_string_free(ptr: *mut c_char) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !ptr.is_null() {
            drop(CString::from_raw(ptr));
        }
    }));
}

fn to_c_string(s: &str) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

fn from_c_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(String::from)
}

// ── Lifecycle functions ─────────────────────────────────────────────

/// Create a new workflow engine instance.
///
/// Supported `workflow_type` values:
/// - `"onboarding"` — onboarding flow (no args)
/// - `"emergency_shred"` — emergency data wipe (no args)
/// - `"lock_screen"` — lock screen with 3 max attempts (no args)
///
/// Returns null on unknown type or null input.
///
/// # Safety
/// `workflow_type` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_workflow_create(
    workflow_type: *const c_char,
) -> *mut VauchiWorkflow {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let wtype = match from_c_str(workflow_type) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };

        let engine: Box<dyn WorkflowEngineAny> = match wtype.as_str() {
            "onboarding" => Box::new(OnboardingEngine::new()),
            "emergency_shred" => Box::new(EmergencyShredEngine::new()),
            "lock_screen" => Box::new(LockScreenEngine::new(3)),
            _ => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiWorkflow {
            engine: Mutex::new(engine),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy a workflow engine instance.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_workflow_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_workflow_destroy(handle: *mut VauchiWorkflow) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }));
}

// ── Screen and action functions ─────────────────────────────────────

/// Get the current screen as a JSON string.
///
/// Returns null if the handle is null. Returns an error JSON object if
/// the internal lock is poisoned. The caller must free the returned
/// string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid workflow handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_workflow_current_screen(
    handle: *mut VauchiWorkflow,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }

        let workflow = &*handle;
        match workflow.engine.lock() {
            Ok(engine) => to_c_string(&engine.current_screen_json()),
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Handle a user action (JSON string) and return the result as JSON.
///
/// Returns null if the handle is null. Returns an error JSON object if
/// the action JSON is null or invalid. The caller must free the returned
/// string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid workflow handle or null.
/// `action_json` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_workflow_handle_action(
    handle: *mut VauchiWorkflow,
    action_json: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }

        let json = match from_c_str(action_json) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null action JSON"}"#),
        };

        let workflow = &*handle;
        match workflow.engine.lock() {
            Ok(mut engine) => to_c_string(&engine.handle_action_json(&json)),
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── AppEngine functions ─────────────────────────────────────────────

/// Opaque handle to an AppEngine instance.
pub struct VauchiApp {
    engine: Mutex<AppEngine>,
}

/// Create a new AppEngine with in-memory storage and default relay.
///
/// Returns null on initialization failure.
///
/// # Safety
/// No special requirements.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_create() -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        vauchi_app_create_with_relay(std::ptr::null())
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new AppEngine with a custom relay URL.
///
/// If `relay_url` is null, uses the default (`wss://relay.vauchi.app`).
/// The caller retains ownership of the `relay_url` string.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `relay_url` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_create_with_relay(relay_url: *const c_char) -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let vauchi = match Vauchi::in_memory() {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };
        let mut engine = AppEngine::new(vauchi);
        if let Some(url) = from_c_str(relay_url) {
            engine.vauchi_mut().config_mut().relay.server_url = url;
        }
        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(engine),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Create a new AppEngine with persistent storage and custom relay URL.
///
/// Unlike `vauchi_app_create` (in-memory), this stores data on disk at
/// `data_dir/vauchi.db`. Pass null for `relay_url` to use the default.
///
/// Returns null on initialization failure.
///
/// # Safety
/// `data_dir` must be a valid null-terminated C string pointing to a
/// writable directory. `relay_url` must be a valid null-terminated C
/// string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_create_with_config(
    data_dir: *const c_char,
    relay_url: *const c_char,
) -> *mut VauchiApp {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = match from_c_str(data_dir) {
            Some(d) => d,
            None => return std::ptr::null_mut(),
        };

        let data_path = std::path::PathBuf::from(&dir);
        if std::fs::create_dir_all(&data_path).is_err() {
            return std::ptr::null_mut();
        }

        let storage_path = data_path.join("vauchi.db");
        let mut config = vauchi_core::api::VauchiConfig::with_storage_path(&storage_path);
        if let Some(url) = from_c_str(relay_url) {
            config = config.with_relay_url(url);
        }

        let vauchi = match Vauchi::new(config) {
            Ok(v) => v,
            Err(_) => return std::ptr::null_mut(),
        };

        Box::into_raw(Box::new(VauchiApp {
            engine: Mutex::new(AppEngine::new(vauchi)),
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy an AppEngine instance.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_app_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_destroy(handle: *mut VauchiApp) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }));
}

/// Get the current screen as a JSON string.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_current_screen(handle: *mut VauchiApp) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = &*handle;
        match app.engine.lock() {
            Ok(engine) => {
                let screen = engine.current_screen();
                match serde_json::to_string(&screen) {
                    Ok(json) => to_c_string(&json),
                    Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                }
            }
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Handle a user action (JSON) and return the result as JSON.
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `action_json` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_handle_action(
    handle: *mut VauchiApp,
    action_json: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let json = match from_c_str(action_json) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null action JSON"}"#),
        };
        let app = &*handle;
        match app.engine.lock() {
            Ok(mut engine) => match serde_json::from_str::<UserAction>(&json) {
                Ok(action) => {
                    let result = engine.handle_action(action);
                    serde_json::to_string(&result).map_or_else(
                        |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                        |j| to_c_string(&j),
                    )
                }
                Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
            },
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Navigate to a screen by name. Returns the new screen as JSON.
///
/// Supported screen names: "home", "contacts", "exchange", "settings",
/// "help", "backup", "lock", "onboarding", "emergency_shred",
/// "device_linking", "duress_pin", "delivery_status".
///
/// # Safety
/// `handle` must be a valid app handle or null.
/// `screen_name` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_navigate_to(
    handle: *mut VauchiApp,
    screen_name: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let name = match from_c_str(screen_name) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null screen name"}"#),
        };
        let screen = match name.as_str() {
            "onboarding" => AppScreen::Onboarding,
            "home" | "my_info" => AppScreen::MyInfo,
            "contacts" => AppScreen::Contacts,
            "exchange" => AppScreen::Exchange,
            "settings" => AppScreen::Settings,
            "help" => AppScreen::Help,
            "backup" => AppScreen::Backup,
            "lock" => AppScreen::Lock,
            "device_linking" => AppScreen::DeviceLinking,
            "duress_pin" => AppScreen::DuressPin,
            "emergency_shred" => AppScreen::EmergencyShred,
            "delivery_status" => AppScreen::DeliveryStatus,
            _ => return to_c_string(&format!(r#"{{"error":"unknown screen: {}"}}"#, name)),
        };
        let app = &*handle;
        match app.engine.lock() {
            Ok(mut engine) => {
                let model = engine.navigate_to(screen);
                serde_json::to_string(&model).map_or_else(
                    |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                    |j| to_c_string(&j),
                )
            }
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get available screens as a JSON array of strings.
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_available_screens(handle: *mut VauchiApp) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = &*handle;
        match app.engine.lock() {
            Ok(engine) => {
                let screens: Vec<&str> = engine
                    .available_screens()
                    .iter()
                    .map(|s| match s {
                        AppScreen::Onboarding => "onboarding",
                        AppScreen::MyInfo => "my_info",
                        AppScreen::Contacts => "contacts",
                        AppScreen::Exchange => "exchange",
                        AppScreen::Settings => "settings",
                        AppScreen::Help => "help",
                        AppScreen::Backup => "backup",
                        AppScreen::Lock => "lock",
                        AppScreen::DeviceLinking => "device_linking",
                        AppScreen::DuressPin => "duress_pin",
                        AppScreen::EmergencyShred => "emergency_shred",
                        AppScreen::DeliveryStatus => "delivery_status",
                        _ => "unknown",
                    })
                    .collect();
                serde_json::to_string(&screens).map_or_else(
                    |e| to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
                    |j| to_c_string(&j),
                )
            }
            Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Returns the default landing screen as a C string ("my_info" or "contacts").
///
/// # Safety
/// `handle` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_app_default_screen(handle: *mut VauchiApp) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let app = &*handle;
        match app.engine.lock() {
            Ok(engine) => {
                let screen_id = match engine.default_screen() {
                    AppScreen::Contacts => "contacts",
                    _ => "my_info",
                };
                to_c_string(screen_id)
            }
            Err(_) => to_c_string("my_info"),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Exchange Session functions ─────────────────────────────────────

/// Wrapper to share a `ManualConfirmationVerifier` via `Arc` while
/// implementing `ProximityVerifier` for the `VerifierChain`.
struct SharedManualVerifier(Arc<ManualConfirmationVerifier>);

impl ProximityVerifier for SharedManualVerifier {
    fn confidence_level(&self) -> ProximityConfidence {
        self.0.confidence_level()
    }
    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.0.emit_challenge(challenge)
    }
    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        self.0.listen_for_response(timeout)
    }
    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        self.0.verify_response(challenge, response)
    }
    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> Result<(), ProximityError> {
        self.0
            .verify_proximity_two_way(emit_challenge, listen_challenge, timeout, is_initiator)
    }
}

/// Opaque handle to an exchange session.
pub struct VauchiExchange {
    session: Mutex<ExchangeSession>,
    manual_verifier: Arc<ManualConfirmationVerifier>,
}

/// Create a new QR exchange session using the app's identity.
///
/// Uses manual confirmation for proximity verification (suitable for
/// desktop platforms without audio proximity hardware).
///
/// Returns null if the app handle is null, identity is not created,
/// or initialization fails.
///
/// # Safety
/// `app` must be a valid app handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_create(app: *mut VauchiApp) -> *mut VauchiExchange {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if app.is_null() {
            return std::ptr::null_mut();
        }
        let app_ref = &*app;
        let engine = match app_ref.engine.lock() {
            Ok(e) => e,
            Err(_) => return std::ptr::null_mut(),
        };

        let vauchi = engine.vauchi();
        let identity_ref = match vauchi.identity() {
            Some(id) => id,
            None => return std::ptr::null_mut(),
        };
        // Clone identity via storage serialization (ExchangeSession needs ownership).
        // Wrap in Zeroizing to scrub master_seed from heap on drop.
        let storage_bytes = Zeroizing::new(identity_ref.to_storage_bytes());
        let identity = match vauchi_core::identity::Identity::from_storage_bytes(&storage_bytes) {
            Ok(id) => id,
            Err(_) => return std::ptr::null_mut(),
        };
        let card = match vauchi.own_card() {
            Ok(Some(c)) => c,
            Ok(None) | Err(_) => return std::ptr::null_mut(),
        };

        let manual = Arc::new(ManualConfirmationVerifier::new());
        let mut chain = VerifierChain::new();
        chain.add(
            VerifierMethod::ManualConfirmation,
            Box::new(SharedManualVerifier(manual.clone())),
        );

        let session = ExchangeSession::new_qr(identity, card, chain);

        Box::into_raw(Box::new(VauchiExchange {
            session: Mutex::new(session),
            manual_verifier: manual,
        }))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Destroy an exchange session.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_exchange_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_destroy(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            drop(Box::from_raw(handle));
        }
    }));
}

/// Start QR generation and return the QR data string ("wb://...").
///
/// Returns error JSON if the session is in the wrong state.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_generate_qr(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        if let Err(e) = session.apply(ExchangeEvent::StartQR) {
            return to_c_string(&format!(r#"{{"error":"{}"}}"#, e));
        }

        match session.qr() {
            Some(qr) => to_c_string(&format!("wb://{}", qr.to_data_string())),
            None => to_c_string(r#"{"error":"QR not generated"}"#),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Process a scanned QR code from the peer.
///
/// `qr_data` should be the full QR string (with or without "wb://" prefix).
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
/// `qr_data` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_process_qr(
    handle: *mut VauchiExchange,
    qr_data: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let data = match from_c_str(qr_data) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null QR data"}"#),
        };

        let data_str = data.strip_prefix("wb://").unwrap_or(&data);
        let qr = match ExchangeQR::from_data_string(data_str) {
            Ok(q) => q,
            Err(_) => return to_c_string(r#"{"error":"invalid QR data"}"#),
        };

        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(ExchangeEvent::ProcessQR(qr)) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Helper: apply a simple event to an exchange session.
unsafe fn exchange_apply_event(handle: *mut VauchiExchange, event: ExchangeEvent) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(event) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Signal that the peer scanned our QR code.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_they_scanned_our_qr(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    exchange_apply_event(handle, ExchangeEvent::TheyScannedOurQR)
}

/// Perform key agreement and proximity verification.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_perform_key_agreement(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    exchange_apply_event(handle, ExchangeEvent::PerformKeyAgreement)
}

/// Complete the exchange with the peer's card name.
///
/// Returns `"ok"` on success, error JSON on failure.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
/// `their_name` must be a valid null-terminated C string, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_complete(
    handle: *mut VauchiExchange,
    their_name: *const c_char,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let name = match from_c_str(their_name) {
            Some(s) => s,
            None => return to_c_string(r#"{"error":"null name"}"#),
        };

        const MAX_NAME_LEN: usize = 256;
        if name.len() > MAX_NAME_LEN {
            return to_c_string(r#"{"error":"name too long"}"#);
        }

        let card = ContactCard::new(&name);
        let exchange = &*handle;
        let mut session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return to_c_string(r#"{"error":"lock poisoned"}"#),
        };

        match session.apply(ExchangeEvent::CompleteExchange(card)) {
            Ok(()) => to_c_string(r#""ok""#),
            Err(e) => to_c_string(&format!(r#"{{"error":"{}"}}"#, e)),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Confirm that the user verified proximity manually.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_confirm_proximity(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            let exchange = &*handle;
            exchange.manual_verifier.confirm();
        }
    }));
}

/// Get the current exchange state as a string label.
///
/// Returns one of: "idle", "displaying_qr", "peer_scanned",
/// "awaiting_key_agreement", "awaiting_card_exchange", "complete", "failed".
/// Returns null if the handle is null.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_state(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        let label = match session.state() {
            ExchangeState::Idle => "idle",
            ExchangeState::DisplayingQr { .. } => "displaying_qr",
            ExchangeState::PeerScanned { .. } => "peer_scanned",
            ExchangeState::AwaitingKeyAgreement { .. } => "awaiting_key_agreement",
            ExchangeState::AwaitingCardExchange { .. } => "awaiting_card_exchange",
            ExchangeState::AwaitingNfcTap => "awaiting_nfc_tap",
            ExchangeState::AwaitingBleConnection => "awaiting_ble_connection",
            ExchangeState::AwaitingBleVerification { .. } => "awaiting_ble_verification",
            ExchangeState::Complete { .. } => "complete",
            ExchangeState::Failed { .. } => "failed",
        };
        to_c_string(label)
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Check whether the exchange session has timed out.
///
/// Returns 1 if timed out, 0 if not, -1 on error.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_is_timed_out(handle: *mut VauchiExchange) -> i32 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return -1;
        }
        let exchange = &*handle;
        match exchange.session.lock() {
            Ok(session) => i32::from(session.is_timed_out()),
            Err(_) => -1,
        }
    }))
    .unwrap_or(-1)
}

/// Get the peer's display name (from their QR code).
///
/// Returns the name string, or null if not yet known or handle is null.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_peer_display_name(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.their_display_name() {
            Some(name) => to_c_string(name),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Enable debug logging on the exchange session.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_enable_debug_log(handle: *mut VauchiExchange) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if !handle.is_null() {
            let exchange = &*handle;
            if let Ok(mut session) = exchange.session.lock() {
                session.enable_debug_log();
            }
        }
    }));
}

/// Get the exchange debug log as JSONL.
///
/// Returns the JSONL string, or null if debug logging is not enabled.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_debug_jsonl(handle: *mut VauchiExchange) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.exchange_debug_log() {
            Some(log) => to_c_string(&log.to_jsonl()),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Get the exchange debug log as Markdown.
///
/// Returns the Markdown string, or null if debug logging is not enabled.
/// Caller must free the returned string with `vauchi_string_free`.
///
/// # Safety
/// `handle` must be a valid exchange handle or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_exchange_debug_markdown(
    handle: *mut VauchiExchange,
) -> *mut c_char {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if handle.is_null() {
            return std::ptr::null_mut();
        }
        let exchange = &*handle;
        let session = match exchange.session.lock() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };

        match session.exchange_debug_log() {
            Some(log) => to_c_string(&log.to_markdown()),
            None => std::ptr::null_mut(),
        }
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: cdylib crate-type prevents integration tests in tests/ directory
#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    // ── Lifecycle tests (Task 12) ───────────────────────────────────

    #[test]
    fn create_onboarding_workflow_returns_non_null_handle() {
        unsafe {
            let wtype = CString::new("onboarding").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(
                !handle.is_null(),
                "onboarding workflow should create successfully"
            );
            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn create_emergency_shred_workflow_returns_non_null_handle() {
        unsafe {
            let wtype = CString::new("emergency_shred").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(
                !handle.is_null(),
                "emergency_shred workflow should create successfully"
            );
            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn create_lock_screen_workflow_returns_non_null_handle() {
        unsafe {
            let wtype = CString::new("lock_screen").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(
                !handle.is_null(),
                "lock_screen workflow should create successfully"
            );
            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn create_unknown_workflow_returns_null() {
        unsafe {
            let wtype = CString::new("nonexistent").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(handle.is_null(), "unknown workflow type should return null");
        }
    }

    #[test]
    fn create_with_null_type_returns_null() {
        unsafe {
            let handle = vauchi_workflow_create(std::ptr::null());
            assert!(handle.is_null(), "null type should return null");
        }
    }

    #[test]
    fn destroy_null_handle_is_safe() {
        // allow(zero_assertions) — this test verifies no crash/UB on null input
        unsafe {
            vauchi_workflow_destroy(std::ptr::null_mut());
        }
    }

    // ── Screen and action tests (Task 13) ───────────────────────────

    #[test]
    fn current_screen_returns_valid_json_with_screen_id() {
        unsafe {
            let wtype = CString::new("onboarding").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(!handle.is_null());

            let json_ptr = vauchi_workflow_current_screen(handle);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap();
            let screen: serde_json::Value = serde_json::from_str(json).unwrap();
            assert!(
                screen.get("screen_id").is_some(),
                "screen should have screen_id"
            );
            assert!(
                screen.get("components").is_some(),
                "screen should have components"
            );
            assert!(screen.get("title").is_some(), "screen should have title");

            vauchi_string_free(json_ptr);
            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn current_screen_with_null_handle_returns_null() {
        unsafe {
            let json_ptr = vauchi_workflow_current_screen(std::ptr::null_mut());
            assert!(json_ptr.is_null());
        }
    }

    #[test]
    fn handle_action_advances_workflow_state() {
        unsafe {
            let wtype = CString::new("onboarding").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(!handle.is_null());

            // Get initial screen (identity_check)
            let screen1_ptr = vauchi_workflow_current_screen(handle);
            let screen1_json = CStr::from_ptr(screen1_ptr).to_str().unwrap().to_string();
            vauchi_string_free(screen1_ptr);

            // Press "create_new" to advance from identity_check to welcome
            let action = CString::new(r#"{"ActionPressed":{"action_id":"create_new"}}"#).unwrap();
            let result_ptr = vauchi_workflow_handle_action(handle, action.as_ptr());
            assert!(!result_ptr.is_null());
            let result_json = CStr::from_ptr(result_ptr).to_str().unwrap();
            // Result should be valid JSON
            let _: serde_json::Value = serde_json::from_str(result_json).unwrap();
            vauchi_string_free(result_ptr);

            // Get screen after action — should be different
            let screen2_ptr = vauchi_workflow_current_screen(handle);
            let screen2_json = CStr::from_ptr(screen2_ptr).to_str().unwrap().to_string();
            vauchi_string_free(screen2_ptr);

            // Screens should differ (workflow advanced)
            assert_ne!(
                screen1_json, screen2_json,
                "screen should change after action"
            );

            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn handle_action_with_invalid_json_returns_error() {
        unsafe {
            let wtype = CString::new("onboarding").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());

            let bad_json = CString::new("not json").unwrap();
            let result_ptr = vauchi_workflow_handle_action(handle, bad_json.as_ptr());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(result.contains("error"), "invalid JSON should return error");

            vauchi_string_free(result_ptr);
            vauchi_workflow_destroy(handle);
        }
    }

    #[test]
    fn handle_action_with_null_handle_returns_null() {
        unsafe {
            let action = CString::new(r#"{"ActionPressed":{"action_id":"test"}}"#).unwrap();
            let result_ptr = vauchi_workflow_handle_action(std::ptr::null_mut(), action.as_ptr());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn handle_action_with_null_json_returns_error() {
        unsafe {
            let wtype = CString::new("onboarding").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());

            let result_ptr = vauchi_workflow_handle_action(handle, std::ptr::null());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(result.contains("error"));

            vauchi_string_free(result_ptr);
            vauchi_workflow_destroy(handle);
        }
    }

    // ── AppEngine tests ─────────────────────────────────────────────

    #[test]
    fn app_create_returns_non_null_handle() {
        unsafe {
            let handle = vauchi_app_create();
            assert!(!handle.is_null(), "app engine should create successfully");
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_destroy_null_is_safe() {
        // allow(zero_assertions): No-panic boundary test — validates null input doesn't crash
        unsafe {
            vauchi_app_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn app_current_screen_returns_onboarding() {
        unsafe {
            let handle = vauchi_app_create();
            let json_ptr = vauchi_app_current_screen(handle);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap();
            let screen: serde_json::Value = serde_json::from_str(json).unwrap();
            assert_eq!(screen["screen_id"], "identity_check");
            vauchi_string_free(json_ptr);
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_create_with_config_returns_non_null_handle() {
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let handle = vauchi_app_create_with_config(dir_cstr.as_ptr(), std::ptr::null());
            assert!(
                !handle.is_null(),
                "app engine with config should create successfully"
            );
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_create_with_config_null_dir_returns_null() {
        unsafe {
            let handle = vauchi_app_create_with_config(std::ptr::null(), std::ptr::null());
            assert!(handle.is_null(), "null data_dir should return null");
        }
    }

    #[test]
    fn app_create_with_config_with_relay_url_returns_non_null() {
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let relay_cstr = CString::new("wss://relay.example.com").unwrap();
            let handle = vauchi_app_create_with_config(dir_cstr.as_ptr(), relay_cstr.as_ptr());
            assert!(
                !handle.is_null(),
                "app engine with config + relay URL should create successfully"
            );
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_create_with_config_persists_across_reopens() {
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();

            // First open — create and complete onboarding
            let handle = vauchi_app_create_with_config(dir_cstr.as_ptr(), std::ptr::null());
            assert!(!handle.is_null());
            vauchi_app_destroy(handle);

            // Second open — should succeed (db file exists)
            let handle2 = vauchi_app_create_with_config(dir_cstr.as_ptr(), std::ptr::null());
            assert!(
                !handle2.is_null(),
                "reopening with same data_dir should succeed"
            );
            vauchi_app_destroy(handle2);
        }
    }

    #[test]
    fn app_available_screens_starts_with_onboarding() {
        unsafe {
            let handle = vauchi_app_create();
            let json_ptr = vauchi_app_available_screens(handle);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap();
            let screens: Vec<String> = serde_json::from_str(json).unwrap();
            assert_eq!(screens, vec!["onboarding"]);
            vauchi_string_free(json_ptr);
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_handle_action_advances_onboarding() {
        unsafe {
            let handle = vauchi_app_create();
            let action = CString::new(r#"{"ActionPressed":{"action_id":"create_new"}}"#).unwrap();
            let result_ptr = vauchi_app_handle_action(handle, action.as_ptr());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            let _: serde_json::Value =
                serde_json::from_str(result).expect("result should be valid JSON");
            vauchi_string_free(result_ptr);
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_navigate_to_unknown_screen_returns_error() {
        unsafe {
            let handle = vauchi_app_create();
            let screen = CString::new("nonexistent").unwrap();
            let json_ptr = vauchi_app_navigate_to(handle, screen.as_ptr());
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap();
            assert!(json.contains("error"), "unknown screen should return error");
            vauchi_string_free(json_ptr);
            vauchi_app_destroy(handle);
        }
    }

    // ── Exchange session tests ──────────────────────────────────────

    /// Drive a VauchiApp through onboarding to create an identity.
    unsafe fn create_app_with_identity() -> *mut VauchiApp {
        let handle = vauchi_app_create();
        assert!(!handle.is_null());

        let steps: &[&str] = &[
            // 1: identity_check → welcome
            r#"{"ActionPressed":{"action_id":"create_new"}}"#,
            // 2: welcome → default_name
            r#"{"ActionPressed":{"action_id":"get_started"}}"#,
            // 3: set display name (on default_name screen)
            r#"{"TextChanged":{"component_id":"display_name","value":"TestUser"}}"#,
            // 4: default_name → skip_gate
            r#"{"ActionPressed":{"action_id":"continue"}}"#,
            // 5: skip_gate → security_explanation (fast path)
            r#"{"ActionPressed":{"action_id":"skip_to_finish"}}"#,
            // 6: security_explanation → backup_prompt
            r#"{"ActionPressed":{"action_id":"continue"}}"#,
            // 7: backup_prompt → ready
            r#"{"ActionPressed":{"action_id":"skip"}}"#,
            // 8: ready → Complete (creates identity)
            r#"{"ActionPressed":{"action_id":"start"}}"#,
        ];

        for step in steps {
            let action = CString::new(*step).unwrap();
            let r = vauchi_app_handle_action(handle, action.as_ptr());
            vauchi_string_free(r);
        }

        handle
    }

    #[test]
    fn exchange_create_returns_null_without_identity() {
        unsafe {
            let app = vauchi_app_create();
            let exchange = vauchi_exchange_create(app);
            assert!(exchange.is_null(), "exchange should fail without identity");
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_create_returns_null_for_null_app() {
        unsafe {
            let exchange = vauchi_exchange_create(std::ptr::null_mut());
            assert!(exchange.is_null());
        }
    }

    #[test]
    fn exchange_destroy_null_is_safe() {
        // allow(zero_assertions) — no-panic boundary test
        unsafe {
            vauchi_exchange_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn exchange_create_with_identity_returns_non_null() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null(), "exchange should succeed with identity");
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_initial_state_is_idle() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let state_ptr = vauchi_exchange_state(exchange);
            assert!(!state_ptr.is_null());
            let state = CStr::from_ptr(state_ptr).to_str().unwrap();
            assert_eq!(state, "idle");

            vauchi_string_free(state_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_state_null_handle_returns_null() {
        unsafe {
            let state_ptr = vauchi_exchange_state(std::ptr::null_mut());
            assert!(state_ptr.is_null());
        }
    }

    #[test]
    fn exchange_generate_qr_transitions_to_displaying() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let qr_ptr = vauchi_exchange_generate_qr(exchange);
            assert!(!qr_ptr.is_null());
            let qr = CStr::from_ptr(qr_ptr).to_str().unwrap();
            assert!(
                qr.starts_with("wb://"),
                "QR should start with wb://, got: {}",
                qr
            );

            let state_ptr = vauchi_exchange_state(exchange);
            let state = CStr::from_ptr(state_ptr).to_str().unwrap();
            assert_eq!(state, "displaying_qr");

            vauchi_string_free(qr_ptr);
            vauchi_string_free(state_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_generate_qr_null_handle_returns_null() {
        unsafe {
            let qr_ptr = vauchi_exchange_generate_qr(std::ptr::null_mut());
            assert!(qr_ptr.is_null());
        }
    }

    #[test]
    fn exchange_is_not_timed_out_initially() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let result = vauchi_exchange_is_timed_out(exchange);
            assert_eq!(result, 0, "new exchange should not be timed out");

            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_is_timed_out_null_returns_error() {
        unsafe {
            let result = vauchi_exchange_is_timed_out(std::ptr::null_mut());
            assert_eq!(result, -1, "null handle should return -1");
        }
    }

    #[test]
    fn exchange_peer_name_null_before_scan() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let name_ptr = vauchi_exchange_peer_display_name(exchange);
            assert!(
                name_ptr.is_null(),
                "peer name should be null before QR scan"
            );

            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_confirm_proximity_null_is_safe() {
        // allow(zero_assertions) — no-panic boundary test
        unsafe {
            vauchi_exchange_confirm_proximity(std::ptr::null_mut());
        }
    }

    #[test]
    fn exchange_debug_log_null_before_enable() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let jsonl_ptr = vauchi_exchange_debug_jsonl(exchange);
            assert!(
                jsonl_ptr.is_null(),
                "debug JSONL should be null before enabling"
            );

            let md_ptr = vauchi_exchange_debug_markdown(exchange);
            assert!(
                md_ptr.is_null(),
                "debug markdown should be null before enabling"
            );

            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_enable_debug_log_produces_output() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            vauchi_exchange_enable_debug_log(exchange);

            // Generate QR to produce at least one debug event
            let qr_ptr = vauchi_exchange_generate_qr(exchange);
            vauchi_string_free(qr_ptr);

            let jsonl_ptr = vauchi_exchange_debug_jsonl(exchange);
            assert!(
                !jsonl_ptr.is_null(),
                "debug JSONL should be non-null after enabling and generating QR"
            );
            let jsonl = CStr::from_ptr(jsonl_ptr).to_str().unwrap();
            assert!(!jsonl.is_empty(), "debug JSONL should not be empty");

            let md_ptr = vauchi_exchange_debug_markdown(exchange);
            assert!(
                !md_ptr.is_null(),
                "debug markdown should be non-null after enabling"
            );

            vauchi_string_free(jsonl_ptr);
            vauchi_string_free(md_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_process_qr_rejects_invalid_data() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            // Must be in DisplayingQr state first
            let qr_ptr = vauchi_exchange_generate_qr(exchange);
            vauchi_string_free(qr_ptr);

            let bad_qr = CString::new("not-a-valid-qr").unwrap();
            let result_ptr = vauchi_exchange_process_qr(exchange, bad_qr.as_ptr());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(
                result.contains("error"),
                "invalid QR should return error, got: {}",
                result
            );

            vauchi_string_free(result_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_process_qr_null_data_returns_error() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let result_ptr = vauchi_exchange_process_qr(exchange, std::ptr::null());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(result.contains("error"));

            vauchi_string_free(result_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_they_scanned_null_returns_null() {
        unsafe {
            let result_ptr = vauchi_exchange_they_scanned_our_qr(std::ptr::null_mut());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn exchange_complete_null_handle_returns_null() {
        unsafe {
            let name = CString::new("Bob").unwrap();
            let result_ptr = vauchi_exchange_complete(std::ptr::null_mut(), name.as_ptr());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn exchange_complete_null_name_returns_error() {
        unsafe {
            let app = create_app_with_identity();
            let exchange = vauchi_exchange_create(app);
            assert!(!exchange.is_null());

            let result_ptr = vauchi_exchange_complete(exchange, std::ptr::null());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(result.contains("error"));

            vauchi_string_free(result_ptr);
            vauchi_exchange_destroy(exchange);
            vauchi_app_destroy(app);
        }
    }
}
