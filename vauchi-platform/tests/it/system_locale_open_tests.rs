// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A phone on "Follow system" pushes only `system_locale` right after the
//! engine opens, before it asks for the first screen. On a French iPhone
//! the app still showed English (vauchi/private#1246).
//!
//! A language Core embeds renders before any shell file or CDN content.

use std::sync::Arc;

use vauchi_platform::PlatformAppEngine;

use crate::support::{FakeKeychain, drive_onboarding, locales_dir};

const RELAY: &str = "https://relay.test";
const FRENCH_SYSTEM: &str = r#"{"system_locale":"fr-CH"}"#;

fn open(dir: &tempfile::TempDir) -> Arc<PlatformAppEngine> {
    PlatformAppEngine::open_with_keychain(
        dir.path().to_string_lossy().to_string(),
        RELAY.into(),
        None,
        Box::new(FakeKeychain::new()),
    )
    .expect("open engine")
}

fn file_value(code: &str, key: &str) -> String {
    let path = locales_dir().join(format!("{code}.json"));
    let bytes = std::fs::read(path).expect("locale catalogue");
    let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    file[key].as_str().expect("key in locale file").to_owned()
}

fn initial_surface(engine: &PlatformAppEngine) -> serde_json::Value {
    let batch = engine.initial_commands_json().unwrap();
    let batch: serde_json::Value = serde_json::from_str(&batch).unwrap();
    batch["commands"]
        .as_array()
        .expect("commands array")
        .iter()
        .find_map(|command| command.get("ReplaceSurface"))
        .expect("initial surface")["surface"]
        .clone()
}

// @internal
#[test]
fn the_first_screen_follows_a_french_system_language() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(&dir);
    engine
        .set_render_context_json(FRENCH_SYSTEM.to_string())
        .unwrap();

    let surface = initial_surface(&engine);
    let title = surface["title"].as_str().expect("surface title");
    assert_eq!(title, file_value("fr", "onboarding.welcome_title"));
    assert_ne!(title, file_value("en", "onboarding.welcome_title"));
}

// @internal
#[test]
fn my_card_after_onboarding_follows_a_french_system_language() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(&dir);
    engine
        .set_render_context_json(FRENCH_SYSTEM.to_string())
        .unwrap();
    drive_onboarding(&engine);

    let surface = initial_surface(&engine);
    let label = surface["nodes"]
        .as_array()
        .expect("surface nodes")
        .iter()
        .find_map(|node| node["Choice"]["label"].as_str())
        .expect("card view-mode choice label");
    assert_eq!(label, file_value("fr", "my_info.view_mode_label"));
    assert_ne!(label, file_value("en", "my_info.view_mode_label"));
}
