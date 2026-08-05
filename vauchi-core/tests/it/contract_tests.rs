// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contract tests: core parsers consume the actual content files from
//! sibling repos (themes/, locales/, networks.json).
//!
//! The ScreenModel/UserAction/ActionResult JSON-schema half of this file
//! retired with the golden scaffolding (ADR-066): those types no longer
//! cross the shell boundary, so their generated schemas and the fixtures
//! validated against them were dead ratchets.

use vauchi_app::theme::load_themes_from_json;
use vauchi_core::social::SocialNetworkRegistry;

// ============================================================
// ============================================================
//
// These tests verify that core's parsers can consume the actual
// content files from sibling repos (themes/, locales/, networks.json).
// A silent parsing failure here means degraded UX for all users.

/// Contract: embedded themes.json parses into multiple themes.
///
/// Catches the silent fallback in get_available_themes() where a parsing
/// failure returns only the default theme instead of failing visibly.
// @scenario: schema_compat :: Core parser accepts themes.json from sibling repo
// @internal
#[test]
fn embedded_themes_json_parses_successfully() {
    // outside the cargo workspace, so a compile-time include escapes the source
    // tree that cargo-mutants relocates (mutation build error). VAUCHI_THEMES_DIR
    // is an absolute path exported by the mutation/CI jobs; fall back to the
    // sibling-repo layout for plain local runs.
    let themes_dir = std::env::var_os("VAUCHI_THEMES_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes"));
    let themes_path = themes_dir.join("generated/themes.json");
    let themes_json = std::fs::read(&themes_path).unwrap_or_else(|e| {
        panic!(
            "themes.json must be readable at {}: {e}",
            themes_path.display()
        )
    });
    let themes = load_themes_from_json(&themes_json)
        .expect("themes.json must parse — silent fallback to default theme is a regression");
    assert!(
        themes.len() >= 2,
        "themes.json should contain multiple themes, got {}",
        themes.len()
    );
}

/// Contract: embedded networks.json parses into a populated registry.
// @scenario: schema_compat :: Core parser accepts networks.json
// @internal
#[test]
fn embedded_networks_json_parses_successfully() {
    let registry = SocialNetworkRegistry::with_defaults();
    assert!(
        registry.all().len() >= 5,
        "networks.json should contain at least 5 social networks, got {}",
        registry.all().len()
    );
}

/// Contract: all locale files in the sibling repo are valid JSON with string values.
// @scenario: schema_compat :: Core parser accepts locale files from sibling repo
// @internal
#[test]
fn locale_files_are_valid_json() {
    let locales_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales");
    if !locales_dir.exists() {
        eprintln!("SKIP: locales/ sibling repo not found");
        return;
    }

    let en_json =
        std::fs::read_to_string(locales_dir.join("en.json")).expect("en.json must be readable");
    let en: serde_json::Value = serde_json::from_str(&en_json).expect("en.json must be valid JSON");
    let en_obj = en.as_object().expect("en.json must be a JSON object");
    assert!(
        en_obj.len() >= 10,
        "en.json should have at least 10 translation keys, got {}",
        en_obj.len()
    );

    for entry in std::fs::read_dir(&locales_dir).expect("locales/ must be readable") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json")
            && !path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().contains("schema"))
        {
            let data = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("{}: {}", path.display(), e));
            let value: serde_json::Value = serde_json::from_str(&data)
                .unwrap_or_else(|e| panic!("{} is invalid JSON: {}", path.display(), e));
            let obj = value
                .as_object()
                .unwrap_or_else(|| panic!("{} must be a JSON object", path.display()));
            assert_eq!(
                obj.len(),
                en_obj.len(),
                "{} has {} keys but en.json has {} — missing or extra translations",
                path.display(),
                obj.len(),
                en_obj.len()
            );
        }
    }
}
