// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Canonical Core presentation reducer C ABI.

use std::os::raw::c_char;

use super::{VauchiApp, from_c_str, to_c_string};

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
            match app.engine.lock() {
                Ok(mut engine) => match engine.initial_commands() {
                    Ok(commands) => {
                        to_c_string(&serde_json::json!({ "commands": commands }).to_string())
                    }
                    Err(error) => {
                        to_c_string(&serde_json::json!({ "error": error.to_string() }).to_string())
                    }
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
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
            let json = match from_c_str(event_json) {
                Some(json) => json,
                None => return to_c_string(r#"{"error":"null event JSON"}"#),
            };
            let event = match serde_json::from_str::<vauchi_core::Event>(&json) {
                Ok(event) => event,
                Err(error) => {
                    return to_c_string(
                        &serde_json::json!({ "error": error.to_string() }).to_string(),
                    );
                }
            };
            let app = &*handle;
            match app.engine.lock() {
                Ok(mut engine) => match engine.dispatch(event) {
                    Ok(commands) => {
                        to_c_string(&serde_json::json!({ "commands": commands }).to_string())
                    }
                    Err(error) => {
                        to_c_string(&serde_json::json!({ "error": error.to_string() }).to_string())
                    }
                },
                Err(_) => to_c_string(r#"{"error":"lock poisoned"}"#),
            }
        }))
        .unwrap_or(std::ptr::null_mut())
    }
}

// INLINE_TEST_REQUIRED: this cdylib/staticlib crate has no Rust integration-test target
#[cfg(test)]
mod tests {
    use std::ffi::CStr;

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
}
