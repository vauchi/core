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
                .lock_engine()
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
            let Ok(mut engine_slot) = app.lock_engine() else {
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

/// Larger than any real context (three short tags); bounds the parse (DC-01).
const MAX_RENDER_CONTEXT_JSON_BYTES: usize = 4 * 1024;

/// Push the shell's render context: the JSON shape of
/// `PlatformAppEngine::set_render_context_json` (`locale`, `system_locale`,
/// `theme_id`). A push while storage is locked is applied when it opens.
///
/// Returns 0 on success, -1 on a null argument or JSON Core rejects, which
/// leaves the current context unchanged.
///
/// # Safety
/// `handle` must be a valid app handle or null. `json` must be a valid
/// null-terminated C string or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn vauchi_app_set_render_context_json(
    handle: *mut VauchiApp,
    json: *const c_char,
) -> i32 {
    unsafe {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if handle.is_null() || json.is_null() {
                return -1;
            }
            let bytes = CStr::from_ptr(json).to_bytes();
            if bytes.len() > MAX_RENDER_CONTEXT_JSON_BYTES {
                return -1;
            }
            let Ok(context) = serde_json::from_slice::<vauchi_app::ui::RenderContext>(bytes) else {
                return -1;
            };
            match (*handle).set_render_context(context) {
                Ok(()) => 0,
                Err(()) => -1,
            }
        }))
        .unwrap_or(-1)
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

    fn set_render_context(app: *mut VauchiApp, json: &str) -> i32 {
        let json = CString::new(json).unwrap();
        // SAFETY: The handle and NUL-terminated JSON are owned by this test.
        unsafe { vauchi_app_set_render_context_json(app, json.as_ptr()) }
    }

    fn resolved_locale(app: *mut VauchiApp) -> vauchi_app::i18n::Locale {
        // SAFETY: The handle is live and owned by this test.
        let app = unsafe { &*app };
        app.lock_engine()
            .unwrap()
            .as_ref()
            .unwrap()
            .render_context()
            .resolved_locale()
    }

    // @internal
    #[test]
    fn c_abi_render_context_carries_the_system_language() {
        // SAFETY: The default constructor has no pointer inputs and returns an owned handle.
        let app = unsafe { crate::vauchi_app_create() };

        assert_eq!(set_render_context(app, r#"{"system_locale":"fr-CH"}"#), 0);

        assert_eq!(resolved_locale(app), vauchi_app::i18n::Locale::FRENCH);
        // SAFETY: The handle is owned by this test and destroyed exactly once.
        unsafe { crate::vauchi_app_destroy(app) };
    }

    // @internal
    #[test]
    fn c_abi_render_context_rejects_bad_input_and_keeps_the_context() {
        // SAFETY: The default constructor has no pointer inputs and returns an owned handle.
        let app = unsafe { crate::vauchi_app_create() };
        assert_eq!(set_render_context(app, r#"{"locale":"de"}"#), 0);
        let oversized = format!(r#"{{"system_locale":"{}"}}"#, "a".repeat(8 * 1024));

        for bad in ["", "{", "[]", r#"{"locale":7}"#, oversized.as_str()] {
            assert_eq!(set_render_context(app, bad), -1, "{bad:.40}");
        }
        // SAFETY: A null JSON pointer is part of the contract under test.
        assert_eq!(
            unsafe { vauchi_app_set_render_context_json(app, std::ptr::null()) },
            -1
        );

        assert_eq!(resolved_locale(app), vauchi_app::i18n::Locale::GERMAN);
        // SAFETY: The handle is owned by this test and destroyed exactly once.
        unsafe { crate::vauchi_app_destroy(app) };
    }

    // @internal
    #[test]
    fn c_abi_render_context_on_a_null_handle_fails() {
        let json = CString::new("{}").unwrap();
        // SAFETY: A null handle is part of the contract under test.
        assert_eq!(
            unsafe { vauchi_app_set_render_context_json(std::ptr::null_mut(), json.as_ptr()) },
            -1
        );
    }
}
