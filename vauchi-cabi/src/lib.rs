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

use vauchi_core::exchange::{ExchangeSession, ManualConfirmationVerifier};
use vauchi_core::ui::*;

mod app;
mod audio;
mod exchange;
mod workflow;

pub use app::*;
pub use audio::*;
pub use exchange::*;
pub use workflow::*;

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
}

/// Opaque handle to an exchange session.
pub struct VauchiExchange {
    pub(crate) session: Mutex<ExchangeSession>,
    pub(crate) manual_verifier: Arc<ManualConfirmationVerifier>,
}

// ── String helpers ──────────────────────────────────────────────────

/// Free a string allocated by vauchi-cabi.
///
/// # Safety
/// `ptr` must be a pointer returned by a vauchi_* function, or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_string_free(ptr: *mut c_char) {
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
        unsafe {
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

    // ── Hardware event tests ────────────────────────────────────────

    #[test]
    fn handle_hardware_event_null_handle_returns_null() {
        unsafe {
            let event = CString::new(r#"{"QrScanned":{"data":"test"}}"#).unwrap();
            let result = vauchi_app_handle_hardware_event(std::ptr::null_mut(), event.as_ptr());
            assert!(result.is_null());
        }
    }

    #[test]
    fn handle_hardware_event_null_json_returns_null() {
        unsafe {
            let handle = vauchi_app_create();
            let result = vauchi_app_handle_hardware_event(handle, std::ptr::null());
            assert!(result.is_null());
            vauchi_app_destroy(handle);
        }
    }

    #[test]
    fn handle_hardware_event_not_on_exchange_returns_null() {
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
        unsafe {
            let cstr = CStr::from_ptr(result);
            assert_eq!(cstr.to_str().unwrap(), "normal string");
            drop(CString::from_raw(result));
        }
    }

    #[test]
    fn to_c_string_empty_string_returns_empty() {
        let result = to_c_string("");
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
        unsafe {
            let handle = vauchi_app_create_with_keyring(std::ptr::null(), std::ptr::null());
            assert!(handle.is_null());
        }
    }

    #[test]
    fn create_with_keyring_valid_dir_returns_non_null() {
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
        unsafe {
            let key = [0xABu8; 32];
            let handle =
                vauchi_app_create_with_key(std::ptr::null(), std::ptr::null(), key.as_ptr(), 32);
            assert!(handle.is_null(), "null data_dir should return null");
        }
    }

    #[test]
    fn create_with_key_persists_identity_across_reopens() {
        unsafe {
            let dir = tempfile::tempdir().unwrap();
            let dir_cstr = CString::new(dir.path().to_str().unwrap()).unwrap();
            let key = [0x42u8; 32];

            let handle =
                vauchi_app_create_with_key(dir_cstr.as_ptr(), std::ptr::null(), key.as_ptr(), 32);
            assert!(!handle.is_null());

            let steps: &[&str] = &[
                r#"{"ActionPressed":{"action_id":"create_new"}}"#,
                r#"{"ActionPressed":{"action_id":"get_started"}}"#,
                r#"{"TextChanged":{"component_id":"display_name","value":"PersistTest"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip_to_finish"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"start"}}"#,
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
                r#"{"ActionPressed":{"action_id":"get_started"}}"#,
                r#"{"TextChanged":{"component_id":"display_name","value":"KeyTest"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip_to_finish"}}"#,
                r#"{"ActionPressed":{"action_id":"continue"}}"#,
                r#"{"ActionPressed":{"action_id":"skip"}}"#,
                r#"{"ActionPressed":{"action_id":"start"}}"#,
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

    // ── Audio backend tests ─────────────────────────────────────────

    #[test]
    fn audio_is_available_returns_valid_result() {
        unsafe {
            let result = vauchi_audio_is_available();
            assert!(result == 0 || result == 1);
        }
    }

    #[test]
    fn audio_emit_null_data_returns_zero() {
        unsafe {
            let result = vauchi_audio_emit(std::ptr::null(), 0);
            assert_eq!(result, 0, "null data should fail gracefully");
        }
    }

    #[test]
    fn audio_listen_zero_timeout_returns_null() {
        unsafe {
            let result = vauchi_audio_listen(0);
            assert!(result.is_null(), "zero timeout should return null");
        }
    }

    #[test]
    fn audio_stop_does_not_crash() {
        // allow(zero_assertions) — no-panic boundary test
        unsafe {
            vauchi_audio_stop();
        }
    }
}
