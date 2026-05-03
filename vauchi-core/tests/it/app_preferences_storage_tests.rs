// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Storage tests for the `app_preferences` table (migration V46).
//! Singleton row backing the user's theme + language picks. Wired
//! into the Settings dropdown by `SettingsEngine` (problem record
//! `2026-05-01-android-humble-ui-deep-retirement` Phase 2a/A3a).

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::types::AppPreferences;

fn temp_storage() -> Storage {
    let key = SymmetricKey::generate();
    Storage::in_memory(key).expect("storage::in_memory")
}

// @internal
#[test]
fn load_returns_default_when_unset() {
    let storage = temp_storage();
    let prefs = storage.load_app_preferences().expect("load");
    assert_eq!(prefs, AppPreferences::default());
    assert_eq!(prefs.theme_id, None);
    assert_eq!(prefs.language_code, None);
    assert!(prefs.follow_system_theme);
    assert!(prefs.follow_system_language);
}

// @internal
#[test]
fn save_then_load_roundtrips() {
    let storage = temp_storage();
    let written = AppPreferences {
        theme_id: Some("ocean-dark".into()),
        language_code: Some("de".into()),
        follow_system_theme: false,
        follow_system_language: false,
    };
    storage.save_app_preferences(&written).expect("save");
    let loaded = storage.load_app_preferences().expect("load");
    assert_eq!(loaded, written);
}

// @internal
#[test]
fn save_overwrites_singleton_row() {
    let storage = temp_storage();
    let first = AppPreferences {
        theme_id: Some("ocean-dark".into()),
        language_code: Some("de".into()),
        follow_system_theme: false,
        follow_system_language: false,
    };
    let second = AppPreferences {
        theme_id: Some("forest-light".into()),
        language_code: None,
        follow_system_theme: false,
        follow_system_language: true,
    };
    storage.save_app_preferences(&first).expect("save first");
    storage.save_app_preferences(&second).expect("save second");
    let loaded = storage.load_app_preferences().expect("load");
    assert_eq!(loaded, second);
}

// @internal
#[test]
fn follow_system_flags_round_trip_independently() {
    let storage = temp_storage();
    let prefs = AppPreferences {
        theme_id: Some("ocean-dark".into()),
        language_code: None,
        follow_system_theme: false,
        follow_system_language: true,
    };
    storage.save_app_preferences(&prefs).expect("save");
    let loaded = storage.load_app_preferences().expect("load");
    assert!(!loaded.follow_system_theme);
    assert!(loaded.follow_system_language);
    assert_eq!(loaded.theme_id.as_deref(), Some("ocean-dark"));
    assert_eq!(loaded.language_code, None);
}
