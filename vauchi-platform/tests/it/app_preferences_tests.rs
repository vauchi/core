// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the `app_preferences` UniFFI surface (theme + language).
//!
//! Phase 2a/A3a of `2026-05-01-android-humble-ui-deep-retirement`:
//! exposes `Vauchi::app_preferences` / `set_app_preferences` to the
//! mobile platforms so frontends can drive Compose theme + locale
//! resolution from the same singleton row that the Settings dropdown
//! intercept writes through.

use std::sync::Arc;

use tempfile::TempDir;

use vauchi_platform::{MobileAppPreferences, VauchiPlatform};

fn setup() -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().unwrap();
    let wb = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .unwrap();
    (wb, dir)
}

// @internal
#[test]
fn app_preferences_default_when_unset() {
    let (wb, _dir) = setup();
    let prefs = wb.app_preferences().expect("load default");
    assert_eq!(prefs.theme_id, None);
    assert_eq!(prefs.language_code, None);
    assert!(prefs.follow_system_theme);
    assert!(prefs.follow_system_language);
}

// @internal
#[test]
fn app_preferences_round_trip_explicit_pick() {
    let (wb, _dir) = setup();
    let written = MobileAppPreferences {
        theme_id: Some("cyber".to_string()),
        language_code: Some("de".to_string()),
        follow_system_theme: false,
        follow_system_language: false,
    };
    wb.set_app_preferences(written.clone()).expect("save");
    let loaded = wb.app_preferences().expect("load");
    assert_eq!(loaded.theme_id, written.theme_id);
    assert_eq!(loaded.language_code, written.language_code);
    assert_eq!(loaded.follow_system_theme, written.follow_system_theme);
    assert_eq!(
        loaded.follow_system_language,
        written.follow_system_language
    );
}

// @internal
#[test]
fn app_preferences_overwrite_replaces_existing() {
    let (wb, _dir) = setup();
    wb.set_app_preferences(MobileAppPreferences {
        theme_id: Some("cyber".to_string()),
        language_code: Some("de".to_string()),
        follow_system_theme: false,
        follow_system_language: false,
    })
    .expect("save first");
    wb.set_app_preferences(MobileAppPreferences {
        theme_id: Some("classic".to_string()),
        language_code: Some("fr".to_string()),
        follow_system_theme: false,
        follow_system_language: false,
    })
    .expect("save second");
    let loaded = wb.app_preferences().expect("load");
    assert_eq!(loaded.theme_id.as_deref(), Some("classic"));
    assert_eq!(loaded.language_code.as_deref(), Some("fr"));
}

// @internal
#[test]
fn app_preferences_follow_system_flags_independent() {
    let (wb, _dir) = setup();
    // User picks an explicit theme but keeps system language.
    wb.set_app_preferences(MobileAppPreferences {
        theme_id: Some("cyber".to_string()),
        language_code: None,
        follow_system_theme: false,
        follow_system_language: true,
    })
    .expect("save");
    let loaded = wb.app_preferences().expect("load");
    assert_eq!(loaded.theme_id.as_deref(), Some("cyber"));
    assert_eq!(loaded.language_code, None);
    assert!(!loaded.follow_system_theme);
    assert!(loaded.follow_system_language);
}

// @internal
#[test]
fn app_preferences_persist_without_identity() {
    // Theme/language preferences must work pre-onboarding (the Settings
    // screen is reachable from the More tab even before identity creation).
    let (wb, _dir) = setup();
    assert!(!wb.has_identity());
    wb.set_app_preferences(MobileAppPreferences {
        theme_id: Some("cyber".to_string()),
        language_code: Some("de".to_string()),
        follow_system_theme: false,
        follow_system_language: false,
    })
    .expect("save without identity");
    let loaded = wb.app_preferences().expect("load without identity");
    assert_eq!(loaded.theme_id.as_deref(), Some("cyber"));
    assert_eq!(loaded.language_code.as_deref(), Some("de"));
}
