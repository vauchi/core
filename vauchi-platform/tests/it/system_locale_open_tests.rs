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

use crate::support::{FakeKeychain, drive_onboarding};

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

// @internal
#[test]
fn the_first_screen_follows_a_french_system_language() {
    let dir = tempfile::tempdir().unwrap();
    let engine = open(&dir);
    engine
        .set_render_context_json(FRENCH_SYSTEM.to_string())
        .unwrap();

    let batch = engine.initial_commands_json().unwrap();

    assert!(batch.contains("\"title\":\"Bienvenue\""), "got {batch}");
    assert!(!batch.contains("\"title\":\"Welcome\""), "got {batch}");
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

    let batch = engine.initial_commands_json().unwrap();

    assert!(batch.contains("Ma Carte"), "got {batch}");
}
