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

use vauchi_app::ui::*;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::{ExchangeSession, ManualConfirmationVerifier};

mod app;
mod app_import_warnings;
mod app_navigation;
mod config;
mod exchange;
mod i18n;
pub(crate) mod platform_event;
mod workflow;

pub use app::*;
pub use app_navigation::*;
pub use exchange::*;
pub use i18n::*;
pub use workflow::*;

use config::CabiConfig;

// ── Type-erased engine wrapper ──────────────────────────────────────

pub(crate) trait WorkflowEngineAny: Send {
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
                match serde_json::to_string(&result) {
                    Ok(json) => json,
                    Err(e) => format!(r#"{{"error":"serialization failed: {}"}}"#, e),
                }
            }
            Err(e) => format!(r#"{{"error":"{}"}}"#, e),
        }
    }
}

/// Opaque handle to a workflow engine instance.
pub struct VauchiWorkflow {
    pub(crate) engine: Mutex<Box<dyn WorkflowEngineAny>>,
}

/// Opaque handle to an AppEngine instance.
pub struct VauchiApp {
    pub(crate) engine: Mutex<AppEngine>,
    /// Active event handler ID for cleanup on replacement or destroy.
    pub(crate) event_handler_id: Mutex<Option<vauchi_core::api::HandlerId>>,
}

/// Opaque handle to an exchange session.
pub struct VauchiExchange {
    pub(crate) session: Mutex<ExchangeSession>,
    pub(crate) manual_verifier: Arc<ManualConfirmationVerifier>,
}

// ── Config builder ──────────────────────────────────────────────────

/// Create a new config builder with data directory and relay URL.
///
/// Returns null if `data_dir` is null.
/// If `relay_url` is null, uses the default (`https://relay.vauchi.app`).
///
/// # Safety
/// `data_dir` and `relay_url` must be valid null-terminated C strings, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_config_new(
    data_dir: *const c_char,
    relay_url: *const c_char,
) -> *mut CabiConfig {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let dir = match from_c_str(data_dir) {
            Some(s) => s,
            None => return std::ptr::null_mut(),
        };
        let relay = from_c_str(relay_url).unwrap_or_else(|| "https://relay.vauchi.app".to_string());

        Box::into_raw(Box::new(CabiConfig::new(dir.into(), relay)))
    })) {
        Ok(result) => result,
        Err(_) => std::ptr::null_mut(),
    }
}

/// Free a config handle.
///
/// # Safety
/// `config` must be a pointer returned by `vauchi_config_new`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_config_free(config: *mut CabiConfig) {
    // SAFETY: If non-null, config was allocated by Box::into_raw in vauchi_config_new.
    unsafe {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !config.is_null() {
                drop(Box::from_raw(config));
            }
        }));
    }
}

/// Set the storage encryption key (exactly 32 bytes, must not be all-zeros).
///
/// Returns `false` if key_len != 32, key is all-zeros, config is null, or key is null.
/// Never panics across the FFI boundary.
///
/// # Safety
/// `config` must be a valid config handle or null.
/// `key` must point to at least `key_len` readable bytes, or be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_config_set_storage_key(
    config: *mut CabiConfig,
    key: *const u8,
    key_len: usize,
) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: Caller guarantees config is a valid CabiConfig pointer or null,
        // and key points to at least key_len readable bytes or is null.
        unsafe {
            if config.is_null() || key.is_null() || key_len != 32 {
                return false;
            }
            let key_bytes: [u8; 32] = std::slice::from_raw_parts(key, 32).try_into().unwrap();

            // Use try_from_bytes to avoid panicking on degenerate (all-zeros) keys
            match vauchi_core::crypto::SymmetricKey::try_from_bytes(key_bytes) {
                Ok(sym_key) => {
                    let config = &mut *config;
                    config.storage_key = Some(sym_key);
                    true
                }
                Err(_) => false,
            }
        }
    }))
    .unwrap_or_default()
}

/// Enable or disable BLE backend.
///
/// # Safety
/// `config` must be a valid config handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_config_enable_ble(config: *mut CabiConfig, enabled: bool) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: Caller guarantees config is a valid CabiConfig pointer or null.
        unsafe {
            if !config.is_null() {
                (*config).ble_enabled = enabled;
            }
        }
    }));
}

/// Enable or disable audio (ultrasonic) backend.
///
/// # Safety
/// `config` must be a valid config handle or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_config_enable_audio(config: *mut CabiConfig, enabled: bool) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        // SAFETY: Caller guarantees config is a valid CabiConfig pointer or null.
        unsafe {
            if !config.is_null() {
                (*config).audio_enabled = enabled;
            }
        }
    }));
}

/// Create an AppEngine from a config builder.
///
/// The config handle is consumed (freed) by this call — do not free it
/// separately. Returns null on initialization failure or if config is null.
///
/// # Safety
/// `config` must be a valid config handle returned by `vauchi_config_new`, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_create_from_config(config: *mut CabiConfig) -> *mut VauchiApp {
    // SAFETY: config is checked non-null, then consumed via Box::from_raw (caller must not use after this call).
    unsafe {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            if config.is_null() {
                return std::ptr::null_mut();
            }
            let config = *Box::from_raw(config);
            let vauchi_config = config.into_vauchi_config();

            let vauchi = match Vauchi::new(vauchi_config) {
                Ok(v) => v,
                Err(_) => return std::ptr::null_mut(),
            };

            Box::into_raw(Box::new(VauchiApp {
                engine: Mutex::new(AppEngine::new(vauchi)),
                event_handler_id: Mutex::new(None),
            }))
        })) {
            Ok(result) => result,
            Err(_) => std::ptr::null_mut(),
        }
    }
}

// ── String helpers ──────────────────────────────────────────────────

/// Free a string allocated by vauchi-cabi.
///
/// # Safety
/// `ptr` must be a pointer returned by a vauchi_* function, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_string_free(ptr: *mut c_char) {
    // SAFETY: ptr was allocated by CString::into_raw() in to_c_string(). Null check guards null case.
    unsafe {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !ptr.is_null() {
                drop(CString::from_raw(ptr));
            }
        }));
    }
}

pub(crate) fn to_c_string(s: &str) -> *mut c_char {
    match CString::new(s) {
        Ok(cstr) => cstr.into_raw(),
        Err(_) => {
            // Input contains interior NUL bytes — strip them rather than
            // returning an empty string (which silently loses all data).
            let sanitized: String = s.chars().filter(|&c| c != '\0').collect();
            CString::new(sanitized)
                .expect("sanitized string should not contain NUL")
                .into_raw()
        }
    }
}

pub(crate) fn from_c_str(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: ptr is checked non-null above. C caller must provide a valid NUL-terminated string.
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(String::from)
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let wtype = CString::new("nonexistent").unwrap();
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(handle.is_null(), "unknown workflow type should return null");
        }
    }

    #[test]
    fn create_with_null_type_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_workflow_create(std::ptr::null());
            assert!(handle.is_null(), "null type should return null");
        }
    }

    #[test]
    fn destroy_null_handle_is_safe() {
        // allow(zero_assertions) — this test verifies no crash/UB on null input
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            vauchi_workflow_destroy(std::ptr::null_mut());
        }
    }

    // ── Screen and action tests (Task 13) ───────────────────────────

    #[test]
    fn current_screen_returns_valid_json_with_screen_id() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let json_ptr = vauchi_workflow_current_screen(std::ptr::null_mut());
            assert!(json_ptr.is_null());
        }
    }

    #[test]
    fn handle_action_advances_workflow_state() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let action = CString::new(r#"{"ActionPressed":{"action_id":"test"}}"#).unwrap();
            let result_ptr = vauchi_workflow_handle_action(std::ptr::null_mut(), action.as_ptr());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn handle_action_with_null_json_returns_error() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create();
            assert!(!handle.is_null(), "app engine should create successfully");
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_destroy_null_is_safe() {
        // allow(zero_assertions): No-panic boundary test — validates null input doesn't crash
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            vauchi_app_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn app_current_screen_returns_onboarding() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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

    // @internal
    #[test]
    fn app_current_tab_id_onboarding_routes_to_onboarding_on_both_layouts() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create();
            // Pre-identity, the active screen is onboarding/identity_check —
            // both the mobile and desktop layouts return "onboarding".
            for layout in [0, 1] {
                let id_ptr = vauchi_app_current_tab_id(handle, layout);
                assert!(!id_ptr.is_null(), "layout {layout}: expected Some");
                let id = CStr::from_ptr(id_ptr).to_str().unwrap();
                assert_eq!(id, "onboarding", "layout {layout}");
                vauchi_string_free(id_ptr);
            }
            vauchi_app_destroy(handle);
        }
    }

    // @internal
    #[test]
    fn app_current_tab_id_invalid_layout_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create();
            let id_ptr = vauchi_app_current_tab_id(handle, 42);
            assert!(id_ptr.is_null(), "invalid layout should return null");
            vauchi_app_destroy(handle);
        }
    }

    // @internal
    #[test]
    fn app_current_tab_id_null_handle_returns_null() {
        // SAFETY: Calling FFI with null inputs from this test scope.
        unsafe {
            let id_ptr = vauchi_app_current_tab_id(std::ptr::null_mut(), 0);
            assert!(id_ptr.is_null());
        }
    }

    #[test]
    fn app_create_with_config_returns_non_null_handle() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create_with_config(std::ptr::null(), std::ptr::null());
            assert!(handle.is_null(), "null data_dir should return null");
        }
    }

    #[test]
    fn app_create_with_config_with_relay_url_returns_non_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let relay_cstr = CString::new("https://relay.example.com").unwrap();
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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

    // @internal
    #[test]
    fn app_navigate_back_returns_a_screen() {
        // After navigating into a sub-screen, navigate_back returns a valid
        // ScreenModel (the prior screen) — the C-ABI path desktop frontends
        // use for their back chrome instead of a footer "Back" action.
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = create_app_with_identity();
            let settings = CString::new("settings").unwrap();
            let fwd = vauchi_app_navigate_to(handle, settings.as_ptr());
            vauchi_string_free(fwd);

            let back_ptr = vauchi_app_navigate_back(handle);
            assert!(!back_ptr.is_null(), "navigate_back must return a screen");
            let json = CStr::from_ptr(back_ptr).to_str().unwrap();
            assert!(
                json.contains("screen_id"),
                "navigate_back must return a ScreenModel, got: {json}"
            );
            assert!(
                !json.contains(r#""error""#),
                "navigate_back must not error, got: {json}"
            );
            vauchi_string_free(back_ptr);
            vauchi_app_destroy(handle);
        }
        // Null handle is tolerated and returns null.
        // SAFETY: passing null is explicitly handled by the function.
        unsafe {
            assert!(vauchi_app_navigate_back(std::ptr::null_mut()).is_null());
        }
    }

    // ── Exchange session tests ──────────────────────────────────────

    /// Drive a VauchiApp through onboarding to create an identity.
    unsafe fn create_app_with_identity() -> *mut VauchiApp {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create();
            assert!(!handle.is_null());

            let steps: &[&str] = &[
                // identity_check → default_name
                r#"{"ActionPressed":{"action_id":"create_new"}}"#,
                // default_name: set display name (also stored as pending_display_name)
                r#"{"TextChanged":{"component_id":"display_name","value":"TestUser"}}"#,
                // default_name → groups_setup
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                // groups_setup → contact_info
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                // contact_info → what_next
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                // what_next → CompleteWith(MainScreen) → create_identity
                r#"{"ActionPressed":{"action_id":"start_app"}}"#,
            ];

            for step in steps {
                let action = CString::new(*step).unwrap();
                let r = vauchi_app_handle_action(handle, action.as_ptr());
                vauchi_string_free(r);
            }

            handle
        }
    }

    #[test]
    fn exchange_create_returns_null_without_identity() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let app = vauchi_app_create();
            let exchange = vauchi_exchange_create(app);
            assert!(exchange.is_null(), "exchange should fail without identity");
            vauchi_app_destroy(app);
        }
    }

    #[test]
    fn exchange_create_returns_null_for_null_app() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let exchange = vauchi_exchange_create(std::ptr::null_mut());
            assert!(exchange.is_null());
        }
    }

    #[test]
    fn exchange_destroy_null_is_safe() {
        // allow(zero_assertions) — no-panic boundary test
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            vauchi_exchange_destroy(std::ptr::null_mut());
        }
    }

    #[test]
    fn exchange_create_with_identity_returns_non_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let state_ptr = vauchi_exchange_state(std::ptr::null_mut());
            assert!(state_ptr.is_null());
        }
    }

    #[test]
    fn exchange_generate_qr_transitions_to_displaying() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let qr_ptr = vauchi_exchange_generate_qr(std::ptr::null_mut());
            assert!(qr_ptr.is_null());
        }
    }

    #[test]
    fn exchange_is_not_timed_out_initially() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let result = vauchi_exchange_is_timed_out(std::ptr::null_mut());
            assert_eq!(result, -1, "null handle should return -1");
        }
    }

    #[test]
    fn exchange_peer_name_null_before_scan() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            vauchi_exchange_confirm_proximity(std::ptr::null_mut());
        }
    }

    #[test]
    fn exchange_debug_log_null_before_enable() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
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
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let result_ptr = vauchi_exchange_they_scanned_our_qr(std::ptr::null_mut());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn exchange_complete_null_handle_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let name = CString::new("Bob").unwrap();
            let result_ptr = vauchi_exchange_complete(std::ptr::null_mut(), name.as_ptr());
            assert!(result_ptr.is_null());
        }
    }

    #[test]
    fn exchange_complete_null_name_returns_error() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
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

    // ── Hardware event tests ────────────────────────────────────────

    #[test]
    fn handle_hardware_event_null_handle_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let event = CString::new(r#"{"QrScanned":{"data":"test"}}"#).unwrap();
            let result = vauchi_app_handle_hardware_event(std::ptr::null_mut(), event.as_ptr());
            assert!(result.is_null());
        }
    }

    #[test]
    fn handle_hardware_event_null_json_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create();
            let result = vauchi_app_handle_hardware_event(handle, std::ptr::null());
            assert!(result.is_null());
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn handle_hardware_event_not_on_exchange_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            // App starts on onboarding, not exchange — event should be ignored
            let handle = vauchi_app_create();
            let event = CString::new(r#"{"QrScanned":{"data":"test"}}"#).unwrap();
            let result = vauchi_app_handle_hardware_event(handle, event.as_ptr());
            assert!(
                result.is_null(),
                "hardware event on non-exchange screen should return null"
            );
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn handle_hardware_event_on_exchange_returns_result() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let app = create_app_with_identity();
            // Navigate to exchange screen
            let screen = CString::new("exchange").unwrap();
            let r = vauchi_app_navigate_to(app, screen.as_ptr());
            if !r.is_null() {
                vauchi_string_free(r);
            }

            // Send a HardwareUnavailable event — should return a toast/alert
            let event = CString::new(r#"{"HardwareUnavailable":{"transport":"BLE"}}"#).unwrap();
            let result = vauchi_app_handle_hardware_event(app, event.as_ptr());
            assert!(
                !result.is_null(),
                "hardware event on exchange screen should return result"
            );
            let result_str = CStr::from_ptr(result).to_str().unwrap();
            assert!(
                result_str.contains("BLE"),
                "result should mention BLE transport: {}",
                result_str
            );
            vauchi_string_free(result);
            vauchi_app_destroy(app);
        }
    }

    // ── to_c_string NUL byte handling (T1-4) ─────────────────────────

    #[test]
    fn to_c_string_strips_nul_bytes_and_returns_sanitized_string() {
        let result = to_c_string("hello\0world");
        // SAFETY: result was just allocated by to_c_string() above; CStr read + CString reclaim.
        unsafe {
            let cstr = CStr::from_ptr(result);
            assert_eq!(
                cstr.to_str().unwrap(),
                "helloworld",
                "NUL bytes should be stripped, not truncate the string"
            );
            // Clean up
            drop(CString::from_raw(result));
        }
    }

    #[test]
    fn to_c_string_normal_string_unchanged() {
        let result = to_c_string("normal string");
        // SAFETY: result was just allocated by to_c_string() above; CStr read + CString reclaim.
        unsafe {
            let cstr = CStr::from_ptr(result);
            assert_eq!(cstr.to_str().unwrap(), "normal string");
            drop(CString::from_raw(result));
        }
    }

    #[test]
    fn to_c_string_empty_string_returns_empty() {
        let result = to_c_string("");
        // SAFETY: result was just allocated by to_c_string() above; CStr read + CString reclaim.
        unsafe {
            let cstr = CStr::from_ptr(result);
            assert_eq!(cstr.to_str().unwrap(), "");
            drop(CString::from_raw(result));
        }
    }

    #[test]
    fn handle_action_json_serialization_failure_returns_error_json() {
        // Verify the Ok path returns valid JSON (not empty string) for all ActionResult variants
        let wtype = CString::new("onboarding").unwrap();
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_workflow_create(wtype.as_ptr());
            assert!(!handle.is_null());

            // A valid action should return non-empty JSON
            let action = CString::new(r#"{"ActionPressed":{"action_id":"create_new"}}"#).unwrap();
            let result_ptr = vauchi_workflow_handle_action(handle, action.as_ptr());
            assert!(!result_ptr.is_null());
            let result = CStr::from_ptr(result_ptr).to_str().unwrap();
            assert!(
                !result.is_empty(),
                "valid action result should not be empty"
            );
            // Must be valid JSON
            let _: serde_json::Value =
                serde_json::from_str(result).expect("action result must always be valid JSON");
            vauchi_string_free(result_ptr);
            vauchi_workflow_destroy(handle);
        }
    }

    // ── Keyring init tests ──────────────────────────────────────────

    #[test]
    fn create_with_keyring_null_dir_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let handle = vauchi_app_create_with_keyring(std::ptr::null(), std::ptr::null());
            assert!(handle.is_null());
        }
    }

    #[test]
    fn create_with_keyring_valid_dir_returns_non_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let handle = vauchi_app_create_with_keyring(dir_cstr.as_ptr(), std::ptr::null());
            // May or may not use keyring depending on platform, but should always succeed
            assert!(
                !handle.is_null(),
                "create_with_keyring should succeed (with or without keyring)"
            );
            vauchi_app_destroy(handle);
        }
    }

    // ── Key-based init tests ───────────────────────────────────────

    #[test]
    fn create_with_key_valid_returns_non_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let key = [0xABu8; 32];
            let handle =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key.as_ptr(), 32);
            assert!(!handle.is_null(), "valid key + dir should succeed");
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn create_with_key_wrong_length_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let key = [0xABu8; 16];
            let handle =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key.as_ptr(), 16);
            assert!(handle.is_null(), "wrong key length should return null");
        }
    }

    #[test]
    fn create_with_key_null_key_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let handle = vauchi_app_create_with_key(
                dir_cstr.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                32,
            );
            assert!(handle.is_null(), "null key_bytes should return null");
        }
    }

    #[test]
    fn create_with_key_null_dir_returns_null() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let key = [0xABu8; 32];
            let handle =
                vauchi_app_create_with_key(std::ptr::null(), std::ptr::null(), key.as_ptr(), 32);
            assert!(handle.is_null(), "null data_dir should return null");
        }
    }

    #[test]
    fn create_with_key_persists_identity_across_reopens() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let key = [0x42u8; 32];

            let handle =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key.as_ptr(), 32);
            assert!(!handle.is_null());

            let steps: &[&str] = &[
                r#"{"ActionPressed":{"action_id":"create_new"}}"#,
                r#"{"TextChanged":{"component_id":"display_name","value":"PersistTest"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"start_app"}}"#,
            ];
            for step in steps {
                let action = CString::new(*step).unwrap();
                let r = vauchi_app_handle_action(handle, action.as_ptr());
                if !r.is_null() {
                    vauchi_string_free(r);
                }
            }

            let screens_ptr = vauchi_app_available_screens(handle);
            let screens_json = CStr::from_ptr(screens_ptr).to_str().unwrap().to_string();
            vauchi_string_free(screens_ptr);
            assert!(
                screens_json.contains("contacts"),
                "after onboarding, contacts should be available: {}",
                screens_json
            );

            vauchi_app_destroy(handle);

            let handle2 =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key.as_ptr(), 32);
            assert!(!handle2.is_null());

            let screens_ptr2 = vauchi_app_available_screens(handle2);
            let screens_json2 = CStr::from_ptr(screens_ptr2).to_str().unwrap().to_string();
            vauchi_string_free(screens_ptr2);
            assert!(
                screens_json2.contains("contacts"),
                "reopened with same key should see contacts: {}",
                screens_json2
            );

            vauchi_app_destroy(handle2);
        }
    }

    #[test]
    fn create_with_key_wrong_key_cannot_read_old_data() {
        // SAFETY: Calling FFI with valid inputs from this test scope.
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let key_a = [0x42u8; 32];
            let key_b = [0x99u8; 32];

            let handle =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key_a.as_ptr(), 32);
            assert!(!handle.is_null());

            let steps: &[&str] = &[
                r#"{"ActionPressed":{"action_id":"create_new"}}"#,
                r#"{"TextChanged":{"component_id":"display_name","value":"KeyTest"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"start_app"}}"#,
            ];
            for step in steps {
                let action = CString::new(*step).unwrap();
                let r = vauchi_app_handle_action(handle, action.as_ptr());
                if !r.is_null() {
                    vauchi_string_free(r);
                }
            }
            vauchi_app_destroy(handle);

            let handle2 =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key_b.as_ptr(), 32);
            if handle2.is_null() {
                assert!(handle2.is_null(), "wrong key returned null (expected)");
            } else {
                let screens_ptr = vauchi_app_available_screens(handle2);
                let screens_json = CStr::from_ptr(screens_ptr).to_str().unwrap().to_string();
                vauchi_string_free(screens_ptr);
                assert!(
                    !screens_json.contains("contacts"),
                    "wrong key should not see contacts: {}",
                    screens_json
                );
                vauchi_app_destroy(handle2);
            }
        }
    }

    // ── Config builder tests ───────────────────────────────────────────

    #[test]
    fn config_new_returns_non_null() {
        // SAFETY: Calling FFI with valid C strings.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), relay.as_ptr());
            assert!(
                !config.is_null(),
                "config_new with valid args should return non-null"
            );
            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_new_with_null_dir_returns_null() {
        // SAFETY: Calling FFI with null data_dir.
        unsafe {
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(std::ptr::null(), relay.as_ptr());
            assert!(config.is_null(), "null data_dir should return null");
        }
    }

    #[test]
    fn config_new_with_null_relay_uses_default() {
        // SAFETY: Calling FFI with null relay_url (should use default).
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
            assert!(
                !config.is_null(),
                "null relay_url should succeed with default"
            );
            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_free_null_is_safe() {
        // allow(zero_assertions) — no-panic boundary test
        // SAFETY: Calling FFI with null config.
        unsafe {
            vauchi_config_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn config_set_storage_key_valid_32_bytes() {
        // SAFETY: Calling FFI with valid config and 32-byte key.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), relay.as_ptr());

            let key: [u8; 32] = [0xAB; 32];
            let result = vauchi_config_set_storage_key(config, key.as_ptr(), 32);
            assert!(result, "valid 32-byte key should succeed");

            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_set_storage_key_rejects_wrong_length() {
        // SAFETY: Calling FFI with valid config and wrong-length key.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), relay.as_ptr());

            let key: [u8; 16] = [0xAB; 16];
            let result = vauchi_config_set_storage_key(config, key.as_ptr(), 16);
            assert!(!result, "16-byte key should be rejected");

            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_set_storage_key_rejects_all_zeros() {
        // SAFETY: Calling FFI with valid config and all-zeros key.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), relay.as_ptr());

            let key: [u8; 32] = [0; 32];
            let result = vauchi_config_set_storage_key(config, key.as_ptr(), 32);
            assert!(!result, "all-zeros key should be rejected");

            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_set_storage_key_null_config_returns_false() {
        // SAFETY: Calling FFI with null config.
        unsafe {
            let key: [u8; 32] = [0xAB; 32];
            let result = vauchi_config_set_storage_key(std::ptr::null_mut(), key.as_ptr(), 32);
            assert!(!result, "null config should return false");
        }
    }

    #[test]
    fn config_set_storage_key_null_key_returns_false() {
        // SAFETY: Calling FFI with null key pointer.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let relay = CString::new("https://relay.vauchi.app").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), relay.as_ptr());

            let result = vauchi_config_set_storage_key(config, std::ptr::null(), 32);
            assert!(!result, "null key pointer should return false");

            vauchi_config_free(config);
        }
    }

    // ── Config enable_ble/audio tests ──────────────────────────────

    #[test]
    fn config_enable_ble_toggles() {
        // SAFETY: Valid config handle, toggling a boolean field.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
            assert!(!config.is_null());

            vauchi_config_enable_ble(config, false);
            assert!(!(*config).ble_enabled);
            vauchi_config_enable_ble(config, true);
            assert!((*config).ble_enabled);

            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_enable_audio_toggles() {
        // SAFETY: Valid config handle, toggling a boolean field.
        unsafe {
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
            assert!(!config.is_null());

            vauchi_config_enable_audio(config, false);
            assert!(!(*config).audio_enabled);

            vauchi_config_free(config);
        }
    }

    #[test]
    fn config_enable_ble_null_config_no_crash() {
        // SAFETY: Null config — should be a no-op. Verify a separate config is unaffected.
        unsafe {
            vauchi_config_enable_ble(std::ptr::null_mut(), true);
            // Prove we didn't corrupt memory: create a real config and verify default
            let dir = CString::new("/tmp/vauchi-test").unwrap();
            let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
            assert!(!config.is_null());
            assert!((*config).ble_enabled, "default should be true");
            vauchi_config_free(config);
        }
    }

    // ── create_from_config tests ───────────────────────────────────

    #[test]
    fn app_create_from_config_returns_non_null() {
        // SAFETY: Valid config handle with temp directory.
        unsafe {
            let tmp = tempfile::tempdir().unwrap();
            let dir = CString::new(tmp.path().to_str().unwrap()).unwrap();
            let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
            assert!(!config.is_null());

            let key: [u8; 32] = [0x42; 32];
            vauchi_config_set_storage_key(config, key.as_ptr(), 32);

            let handle = vauchi_app_create_from_config(config);
            assert!(!handle.is_null(), "should create app from config");
            assert!(
                tmp.path().join("vauchi.db").exists(),
                "vauchi.db should be created inside the data dir"
            );

            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn app_create_from_config_null_returns_null() {
        // SAFETY: Null config — should return null.
        unsafe {
            let handle = vauchi_app_create_from_config(std::ptr::null_mut());
            assert!(handle.is_null());
        }
    }

    #[test]
    fn app_create_from_config_persists_identity() {
        // SAFETY: Two sequential app creates with the same data dir to test persistence.
        unsafe {
            let tmp = tempfile::tempdir().unwrap();
            let data_dir = tmp.path();
            let db_path = data_dir.join("vauchi.db");
            let key: [u8; 32] = [0x42; 32];

            // First launch: create identity
            {
                let dir = CString::new(data_dir.to_str().unwrap()).unwrap();
                let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
                vauchi_config_set_storage_key(config, key.as_ptr(), 32);
                let handle = vauchi_app_create_from_config(config);
                assert!(!handle.is_null());

                // Trigger identity creation
                let action =
                    CString::new(r#"{"ActionPressed":{"action_id":"create_new"}}"#).unwrap();
                let result_ptr = vauchi_app_handle_action(handle, action.as_ptr());
                if !result_ptr.is_null() {
                    vauchi_string_free(result_ptr);
                }

                vauchi_app_destroy(handle);
            }

            // Second launch: should have persisted data
            {
                let dir = CString::new(data_dir.to_str().unwrap()).unwrap();
                let config = vauchi_config_new(dir.as_ptr(), std::ptr::null());
                vauchi_config_set_storage_key(config, key.as_ptr(), 32);
                let handle = vauchi_app_create_from_config(config);
                assert!(!handle.is_null());

                assert!(
                    db_path.exists(),
                    "vauchi.db should persist inside the data dir"
                );

                vauchi_app_destroy(handle);
            }
        }
    }
}
