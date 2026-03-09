// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI bindings for vauchi-core workflow engines.
//!
//! Consumed by Windows (C#/P/Invoke) and Linux-Qt (C++/QJsonDocument).
//! All data exchange uses JSON strings.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::Mutex;

use vauchi_core::ui::*;

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
    if !ptr.is_null() {
        drop(CString::from_raw(ptr));
    }
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
}

/// Destroy a workflow engine instance.
///
/// # Safety
/// `handle` must be a pointer returned by `vauchi_workflow_create`, or null.
#[no_mangle]
pub unsafe extern "C" fn vauchi_workflow_destroy(handle: *mut VauchiWorkflow) {
    if !handle.is_null() {
        drop(Box::from_raw(handle));
    }
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
    if handle.is_null() {
        return std::ptr::null_mut();
    }

    let workflow = &*handle;
    match workflow.engine.lock() {
        Ok(engine) => to_c_string(&engine.current_screen_json()),
        Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
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
}
