// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! C ABI shells get CDN content without scheduling it: cached content loads
//! when the app opens, and Core runs the update cycle within the person's
//! setting (vauchi/private#733 D10). The string store is process-global;
//! nextest runs each test in its own process.

use std::ffi::CString;

use crate::VauchiApp;
use crate::app::{vauchi_app_create, vauchi_app_create_with_config, vauchi_app_destroy};

fn open_on(dir: &tempfile::TempDir) -> *mut VauchiApp {
    let path = CString::new(dir.path().to_str().unwrap()).unwrap();
    // SAFETY: `path` is a valid C string; a null relay URL means the default.
    let app = unsafe { vauchi_app_create_with_config(path.as_ptr(), std::ptr::null()) };
    assert!(!app.is_null());
    app
}

fn destroy(app: *mut VauchiApp) {
    // SAFETY: `app` came from a vauchi_app_create* call and is not used again.
    unsafe { vauchi_app_destroy(app) };
}

// @internal
#[test]
fn a_locale_cached_by_an_earlier_run_is_loaded_when_the_app_opens() {
    use vauchi_app::content::{ContentCache, ContentType, compute_checksum};
    let dir = tempfile::tempdir().unwrap();
    let cached = br#"{"_meta":{"locale":"de"},"cdn.probe":"vom CDN"}"#;
    ContentCache::new(dir.path())
        .unwrap()
        .save_content(
            ContentType::Locales,
            "de.json",
            cached,
            &compute_checksum(cached),
        )
        .unwrap();

    let app = open_on(&dir);

    assert_eq!(
        vauchi_app::i18n::try_get_string(vauchi_app::i18n::Locale::GERMAN, "cdn.probe").as_deref(),
        Some("vom CDN")
    );
    destroy(app);
}

// @internal
#[test]
fn a_scheduled_cycle_with_content_updates_off_makes_no_request() {
    let dir = tempfile::tempdir().unwrap();
    let app = open_on(&dir);
    // SAFETY: `app` is a live handle from open_on.
    let handle = unsafe { &*app };
    handle
        .engine
        .lock()
        .unwrap()
        .as_mut()
        .unwrap()
        .vauchi_mut()
        .config_mut()
        .content_updates = vauchi_core::settings::ContentUpdates::Off;

    let ran = handle
        .scheduled_content_cycle()
        .and_then(|cycle| cycle.run());

    assert_eq!(ran, None);
    assert!(!dir.path().join("content").join("last_check").exists());
    destroy(app);
}

// @internal
#[test]
fn an_app_without_a_data_directory_has_no_content_cycle() {
    // SAFETY: no arguments.
    let app = unsafe { vauchi_app_create() };
    // SAFETY: `app` is a live handle.
    let handle = unsafe { &*app };

    assert!(handle.scheduled_content_cycle().is_none());
    destroy(app);
}
