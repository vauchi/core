// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Workflow engine C ABI functions.

use std::os::raw::c_char;

use vauchi_app::ui::*;

use super::{VauchiWorkflow, WorkflowEngineAny, from_c_str, to_c_string};

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
#[unsafe(no_mangle)]
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
            engine: std::sync::Mutex::new(engine),
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_workflow_destroy(handle: *mut VauchiWorkflow) {
    // SAFETY: ptr was created by Box::into_raw in vauchi_workflow_create. Caller must not use the handle after this call.
    unsafe {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if !handle.is_null() {
                drop(Box::from_raw(handle));
            }
        }));
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_workflow_current_screen(
    handle: *mut VauchiWorkflow,
) -> *mut c_char {
    // SAFETY: handle is checked non-null. Created by Box::into_raw and not yet freed.
    unsafe {
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_workflow_handle_action(
    handle: *mut VauchiWorkflow,
    action_json: *const c_char,
) -> *mut c_char {
    // SAFETY: handle and action_json are checked non-null. C caller must provide a valid NUL-terminated string.
    unsafe {
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
}
