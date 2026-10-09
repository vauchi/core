// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical Core presentation reducer C ABI.

use std::ffi::CStr;
use std::os::raw::c_char;

use super::{VauchiApp, to_c_string};

/// Return the versioned Core presentation contract corpus as canonical JSON.
///
/// The returned string must be released with `vauchi_string_free`.
#[unsafe(no_mangle)]
pub extern "C" fn vauchi_presentation_contract_fixture() -> *mut c_char {
    std::panic::catch_unwind(|| to_c_string(vauchi_app::ui::presentation_contract_fixture_json()))
        .unwrap_or(std::ptr::null_mut())
}

/// Return the complete initial Core command batch.
///
/// # Safety
/// `handle` must be a valid app handle or null. The returned string must be
/// released with `vauchi_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_initial_commands(handle: *mut VauchiApp) -> *mut c_char {
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            let app = &*handle;
            if let Some(commands) = app.locked_initial_commands() {
                return to_c_string(&serde_json::json!({ "commands": commands }).to_string());
            }
            match app
                .engine
                .lock()
                .ok()
                .as_mut()
                .and_then(|slot| slot.as_mut())
            {
                Some(engine) => match engine.initial_commands() {
                    Ok(commands) => {
                        to_c_string(&serde_json::json!({ "commands": commands }).to_string())
                    }
                    Err(error) => {
                        to_c_string(&serde_json::json!({ "error": error.to_string() }).to_string())
                    }
                },
                None => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        }))
        .unwrap_or(std::ptr::null_mut())
    }
}

/// Reduce one canonical event into an ordered Core command batch.
///
/// # Safety
/// `handle` must be a valid app handle or null. `event_json` must be a valid
/// null-terminated C string or null. The returned string must be released with
/// `vauchi_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_dispatch(
    handle: *mut VauchiApp,
    event_json: *const c_char,
) -> *mut c_char {
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() {
                return std::ptr::null_mut();
            }
            if event_json.is_null() {
                return to_c_string(r#"{"error":"null event JSON"}"#);
            }
            let app = &*handle;
            let locked_event = CStr::from_ptr(event_json)
                .to_str()
                .map_err(|_| vauchi_core::EventJsonError::Malformed)
                .and_then(vauchi_core::event_from_json);
            let locked_answer = match locked_event {
                Ok(event) => app.locked_dispatch(event),
                Err(error) => app.locked_reject_event_json(&error),
            };
            if let Some(commands) = locked_answer {
                return to_c_string(&serde_json::json!({ "commands": commands }).to_string());
            }
            let Ok(mut engine_slot) = app.engine.lock() else {
                return to_c_string(r#"{"error":"lock poisoned"}"#);
            };
            let Some(engine) = engine_slot.as_mut() else {
                return to_c_string(r#"{"error":"lock poisoned"}"#);
            };
            let decoded = CStr::from_ptr(event_json)
                .to_str()
                .map_err(|_| vauchi_core::EventJsonError::Malformed)
                .and_then(vauchi_core::event_from_json);
            let commands = match decoded {
                Ok(event) => engine
                    .dispatch(event)
                    .unwrap_or_else(|rejection| engine.reject_dispatch(&rejection)),
                Err(error) => engine.reject_event_json(&error),
            };
            to_c_string(&serde_json::json!({ "commands": commands }).to_string())
        }))
        .unwrap_or(std::ptr::null_mut())
    }
}

// INLINE_TEST_REQUIRED: this cdylib/staticlib crate has no Rust integration-test target
#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString};

    use super::*;

    // @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
    #[test]
    fn c_abi_returns_the_core_owned_fixture_bytes() {
        let fixture_ptr = vauchi_presentation_contract_fixture();
        assert!(!fixture_ptr.is_null());
        // SAFETY: The C ABI returned a valid string owned by this test.
        let fixture = unsafe { CStr::from_ptr(fixture_ptr) }.to_str().unwrap();
        assert_eq!(
            fixture,
            vauchi_app::ui::presentation_contract_fixture_json()
        );
        // SAFETY: The pointer is owned by the C ABI and freed exactly once.
        unsafe { crate::vauchi_string_free(fixture_ptr) };
    }

    /// Dispatch one event through a fresh app handle and return the raw reply.
    fn dispatch_to_json(event_json: &str) -> String {
        // SAFETY: The default constructor has no pointer inputs and returns an owned handle.
        let app = unsafe { crate::vauchi_app_create() };
        assert!(!app.is_null());
        let event = CString::new(event_json).unwrap();
        // SAFETY: The handle and NUL-terminated event string are owned by this test.
        let response_ptr = unsafe { vauchi_app_dispatch(app, event.as_ptr()) };
        assert!(!response_ptr.is_null());
        // SAFETY: The C ABI returned a valid string owned by this test.
        let response = unsafe { CStr::from_ptr(response_ptr) }
            .to_str()
            .unwrap()
            .to_owned();
        // SAFETY: Both pointers are owned by the C ABI and freed exactly once.
        unsafe {
            crate::vauchi_string_free(response_ptr);
            crate::vauchi_app_destroy(app);
        }
        response
    }

    /// A rejection answers with Core-prepared copy, never its own text
    /// (ADR-045 Am1, DC-05).
    fn assert_prepared_rejection_alert(response: &str, rejection_text: &str) {
        let batch: serde_json::Value = serde_json::from_str(response).unwrap();
        let alert = &batch["commands"][0]["PresentAlert"]["alert"];
        assert_eq!(alert["title"], "Error", "prepared title, got {batch}");
        assert_eq!(
            alert["message"], "Something went wrong",
            "prepared message, got {batch}"
        );
        assert!(
            !response.contains(rejection_text),
            "the rejection's own text must not reach the shell, got {batch}"
        );
    }

    /// Feature: generic_presentation_protocol.feature
    /// Scenario: Invalid boundary input fails safely
    // @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
    #[test]
    fn c_abi_explains_oversized_event_json_with_a_prepared_alert() {
        let response = dispatch_to_json(&format!(
            "\"PresentationInvalidated\"{padding}",
            padding = " ".repeat(vauchi_core::MAX_EVENT_JSON_BYTES)
        ));

        assert_prepared_rejection_alert(&response, "exceeds");
    }

    // @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
    #[test]
    fn c_abi_explains_an_event_for_an_inactive_surface_with_a_prepared_alert() {
        let response = dispatch_to_json(
            &serde_json::json!({
                "ValueChanged": {
                    "surface_id": "surface",
                    "binding_id": "binding",
                    "value": { "text": "short" },
                }
            })
            .to_string(),
        );

        assert_prepared_rejection_alert(&response, "surface is not active");
    }

    // @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
    #[test]
    fn c_abi_explains_an_oversized_input_value_with_an_alert() {
        // SAFETY: The default constructor has no pointer inputs and returns an owned handle.
        let app = unsafe { crate::vauchi_app_create() };
        assert!(!app.is_null());
        let event = CString::new(
            serde_json::json!({
                "ValueChanged": {
                    "surface_id": "surface",
                    "binding_id": "binding",
                    "value": { "text": "a".repeat(vauchi_core::MAX_EVENT_INPUT_VALUE_BYTES + 1) },
                }
            })
            .to_string(),
        )
        .unwrap();

        // SAFETY: The handle and NUL-terminated event string are owned by this test.
        let response_ptr = unsafe { vauchi_app_dispatch(app, event.as_ptr()) };
        assert!(!response_ptr.is_null());
        // SAFETY: The C ABI returned a valid string owned by this test.
        let response = unsafe { CStr::from_ptr(response_ptr) }.to_str().unwrap();
        let response: serde_json::Value = serde_json::from_str(response).unwrap();
        let message = response["commands"][0]["PresentAlert"]["alert"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("expected a PresentAlert batch, got {response}"));
        assert!(message.contains(&vauchi_core::MAX_EVENT_INPUT_VALUE_BYTES.to_string()));

        // SAFETY: Both pointers are owned by the C ABI and freed exactly once.
        unsafe {
            crate::vauchi_string_free(response_ptr);
            crate::vauchi_app_destroy(app);
        }
    }
}
