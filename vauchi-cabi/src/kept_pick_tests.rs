// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! C ABI shells get the language and theme pick without storing it: Core
//! keeps it beside its content cache and applies it when the app opens
//! (vauchi/private#814). The theme store is process-global; nextest runs
//! each test in its own process.

use std::ffi::CString;

use vauchi_core::Command;

use crate::app::{vauchi_app_create_with_config, vauchi_app_destroy};

/// The file Core writes there (vauchi/private#814), standing in for a
/// Settings pick of an earlier run.
fn keep_latte_from_an_earlier_run(dir: &std::path::Path) {
    std::fs::write(
        dir.join("restorable_state"),
        r#"{"v":1,"locale":null,"theme_id":"catppuccin-latte"}"#,
    )
    .unwrap();
}

// @internal
#[test]
fn opening_applies_the_pick_core_kept_in_the_data_directory() {
    let dir = tempfile::tempdir().unwrap();
    keep_latte_from_an_earlier_run(dir.path());
    let path = CString::new(dir.path().to_str().unwrap()).unwrap();

    // SAFETY: `path` is a valid C string; a null relay URL means the default.
    let app = unsafe { vauchi_app_create_with_config(path.as_ptr(), std::ptr::null()) };
    assert!(!app.is_null());
    // SAFETY: `app` is a live handle from vauchi_app_create_with_config.
    let handle = unsafe { &*app };
    let background = handle
        .engine
        .lock()
        .unwrap()
        .as_mut()
        .expect("storage opened")
        .initial_commands()
        .unwrap()
        .into_iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.tokens.colors.background),
            _ => None,
        })
        .expect("a surface");

    assert_eq!(background, "#eff1f5");
    // SAFETY: `app` came from vauchi_app_create_with_config and is not used again.
    unsafe { vauchi_app_destroy(app) };
}
