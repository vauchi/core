// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! C ABI bindings for vauchi-core workflow engines.
//!
//! Consumed by Windows (C#/P/Invoke) and Linux-Qt (C++/QJsonDocument).
//! All data exchange uses JSON strings.

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

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
