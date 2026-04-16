// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden JSON fixtures for onboarding screens.
//!
//! Generates canonical JSON for each of the 6 onboarding screens, consumed
//! by frontend contract tests.
//!
//! Verify freshness: `cargo test -p vauchi-core --test golden_fixtures`
//! Regenerate all:   `cargo test -p vauchi-core --test golden_fixtures -- --ignored`

use std::fs;
use std::path::PathBuf;
use vauchi_app::ui::{
    ActionResult, CURRENT_SCHEMA_VERSION, OnboardingEngine, ScreenModel, UserAction, WorkflowEngine,
};

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

fn screen_to_json(screen: &ScreenModel) -> String {
    serde_json::to_string_pretty(screen).expect("ScreenModel serialization failed")
}

/// Asserts that the golden fixture file matches the current screen JSON.
/// If the file does not exist yet, generates it (bootstrap mode).
fn assert_fixture_fresh(screen: &ScreenModel, filename: &str) {
    let json = screen_to_json(screen);
    let path = fixtures_dir().join(filename);

    if path.exists() {
        let existing = fs::read_to_string(&path).unwrap();
        // Normalize CRLF → LF so fixtures work on any OS/git checkout config.
        assert_eq!(
            existing.replace("\r\n", "\n").trim(),
            json.trim(),
            "Golden fixture `{}` is stale! Regenerate with:\n  \
             cargo test -p vauchi-core --test golden_fixtures -- --ignored",
            filename
        );
    } else {
        // First run — generate the fixture
        fs::create_dir_all(fixtures_dir()).unwrap();
        fs::write(&path, &json).unwrap();
    }
}

/// Walk through all 6 screens, collecting each ScreenModel.
/// Returns `(screen_id, ScreenModel)` pairs in order.
fn walk_all_screens() -> Vec<(String, ScreenModel)> {
    let mut engine = OnboardingEngine::new();
    let mut screens = Vec::new();

    // 1. IdentityCheck
    let screen = engine.current_screen();
    screens.push(("identity_check".to_string(), screen));

    // Navigate: IdentityCheck -> LinkChoice (via "have_identity")
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "have_identity".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 2. LinkChoice
    let screen = engine.current_screen();
    screens.push(("link_choice".to_string(), screen));

    // Navigate: LinkChoice -> IdentityCheck (via "back"), then IdentityCheck -> DefaultName (via "create_new")
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "create_new".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 3. DefaultName (empty — captures the initial state)
    let screen = engine.current_screen();
    screens.push(("default_name".to_string(), screen));

    // Enter a name, then advance
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Alice".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 4. GroupsSetup
    let screen = engine.current_screen();
    screens.push(("groups_setup".to_string(), screen));

    // Advance: continue (no groups toggled — default state)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 5. ContactInfo
    let screen = engine.current_screen();
    screens.push(("contact_info".to_string(), screen));

    // Advance: continue (no fields added — default state)
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 6. WhatNext
    let screen = engine.current_screen();
    screens.push(("what_next".to_string(), screen));

    assert_eq!(screens.len(), 6, "expected exactly 6 onboarding screens");
    screens
}

// ── Per-screen freshness tests ─────────────────────────────────────

// @internal
#[test]
fn identity_check_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[0];
    assert_fixture_fresh(screen, "identity_check.json");
}

// @internal
#[test]
fn link_choice_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[1];
    assert_fixture_fresh(screen, "link_choice.json");
}

// @internal
#[test]
fn default_name_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[2];
    assert_fixture_fresh(screen, "default_name.json");
}

// @internal
#[test]
fn groups_setup_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[3];
    assert_fixture_fresh(screen, "groups_setup.json");
}

// @internal
#[test]
fn contact_info_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[4];
    assert_fixture_fresh(screen, "contact_info.json");
}

// @internal
#[test]
fn what_next_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[5];
    assert_fixture_fresh(screen, "what_next.json");
}

// ── Version metadata ──────────────────────────────────────────────

/// Writes a `.version` metadata file alongside golden fixtures.
/// Frontend contract tests use this to verify fixture/binding version alignment.
fn write_version_file(fixture_count: usize) {
    let meta = serde_json::json!({
        "core_version": env!("CARGO_PKG_VERSION"),
        "schema_version": CURRENT_SCHEMA_VERSION,
        "fixture_count": fixture_count,
    });
    let content = serde_json::to_string_pretty(&meta).unwrap() + "\n";
    fs::write(fixtures_dir().join(".version"), content).unwrap();
}

// @internal
#[test]
fn version_metadata_file_exists_and_is_valid() {
    let path = fixtures_dir().join(".version");
    assert!(path.exists(), ".version file missing — regenerate fixtures");

    let content = fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value =
        serde_json::from_str(&content).expect(".version is not valid JSON");

    assert_eq!(
        parsed["core_version"].as_str().unwrap(),
        env!("CARGO_PKG_VERSION"),
        ".version core_version mismatch"
    );
    assert_eq!(
        parsed["schema_version"].as_u64().unwrap(),
        u64::from(CURRENT_SCHEMA_VERSION),
        ".version schema_version mismatch"
    );

    // fixture_count must match actual .json files
    let json_count = fs::read_dir(fixtures_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .count();
    assert_eq!(
        parsed["fixture_count"].as_u64().unwrap(),
        json_count as u64,
        ".version fixture_count does not match actual .json file count"
    );
}

// ── Regenerate all fixtures (run with --ignored) ───────────────────

/// Regenerate all golden fixtures and the `.version` metadata file.
/// Run with: `cargo test -p vauchi-core --test golden_fixtures -- --ignored`
// @internal
#[test]
#[ignore]
fn regenerate_all_fixtures() {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).unwrap();

    // Remove stale fixture files for removed screens
    let stale_files = [
        "welcome.json",
        "skip_gate.json",
        "preview_card.json",
        "security_explanation.json",
        "backup_prompt.json",
        "ready.json",
    ];
    for stale in &stale_files {
        let path = dir.join(stale);
        if path.exists() {
            fs::remove_file(&path).unwrap();
            println!("Removed stale fixture: {stale}");
        }
    }

    let screens = walk_all_screens();
    assert_eq!(screens.len(), 6, "expected 6 onboarding screens");

    for (name, screen) in &screens {
        let filename = format!("{name}.json");
        let json = screen_to_json(screen);
        fs::write(dir.join(&filename), &json).unwrap();
        println!("Generated {filename}");
    }

    // Count all .json files (including engine fixtures)
    let json_count = fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .count();
    write_version_file(json_count);
    println!("Generated .version (count={json_count})");
}
