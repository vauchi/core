// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Golden JSON fixtures for onboarding screens.
//!
//! Generates canonical JSON for each of the 9 onboarding screens, consumed
//! by frontend contract tests.
//!
//! Verify freshness: `cargo test -p vauchi-core --test golden_fixtures`
//! Regenerate all:   `cargo test -p vauchi-core --test golden_fixtures -- --ignored`

use std::fs;
use std::path::PathBuf;
use vauchi_core::ui::{ActionResult, OnboardingEngine, ScreenModel, UserAction, WorkflowEngine};

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
        assert_eq!(
            existing.trim(),
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

/// Walk through all 9 screens, collecting each ScreenModel.
/// Returns `(screen_id, ScreenModel)` pairs in order.
fn walk_all_screens() -> Vec<(String, ScreenModel)> {
    let mut engine = OnboardingEngine::new();
    let mut screens = Vec::new();

    // 1. Welcome
    let screen = engine.current_screen();
    screens.push(("welcome".to_string(), screen));

    // Advance: Welcome -> DefaultName
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "get_started".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 2. DefaultName (empty — captures the initial state)
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

    // 3. SkipGate
    let screen = engine.current_screen();
    screens.push(("skip_gate".to_string(), screen));

    // Advance: continue setup
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue_setup".into(),
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

    // 6. PreviewCard
    let screen = engine.current_screen();
    screens.push(("preview_card".to_string(), screen));

    // Advance: continue
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 7. SecurityExplanation
    let screen = engine.current_screen();
    screens.push(("security_explanation".to_string(), screen));

    // Advance: continue
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "continue".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 8. BackupPrompt
    let screen = engine.current_screen();
    screens.push(("backup_prompt".to_string(), screen));

    // Advance: skip
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "skip".into(),
    });
    assert!(matches!(result, ActionResult::NavigateTo(_)));

    // 9. Ready
    let screen = engine.current_screen();
    screens.push(("ready".to_string(), screen));

    assert_eq!(screens.len(), 9, "expected exactly 9 onboarding screens");
    screens
}

// ── Per-screen freshness tests ─────────────────────────────────────

#[test]
fn welcome_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[0];
    assert_fixture_fresh(screen, "welcome.json");
}

#[test]
fn default_name_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[1];
    assert_fixture_fresh(screen, "default_name.json");
}

#[test]
fn skip_gate_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[2];
    assert_fixture_fresh(screen, "skip_gate.json");
}

#[test]
fn groups_setup_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[3];
    assert_fixture_fresh(screen, "groups_setup.json");
}

#[test]
fn contact_info_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[4];
    assert_fixture_fresh(screen, "contact_info.json");
}

#[test]
fn preview_card_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[5];
    assert_fixture_fresh(screen, "preview_card.json");
}

#[test]
fn security_explanation_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[6];
    assert_fixture_fresh(screen, "security_explanation.json");
}

#[test]
fn backup_prompt_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[7];
    assert_fixture_fresh(screen, "backup_prompt.json");
}

#[test]
fn ready_fixture_is_fresh() {
    let screens = walk_all_screens();
    let (_, screen) = &screens[8];
    assert_fixture_fresh(screen, "ready.json");
}

// ── Regenerate all fixtures (run with --ignored) ───────────────────

/// Regenerate all golden fixtures.
/// Run with: `cargo test -p vauchi-core --test golden_fixtures -- --ignored`
// allow(zero_assertions)
#[test]
#[ignore]
fn regenerate_all_fixtures() {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).unwrap();

    let screens = walk_all_screens();
    for (name, screen) in &screens {
        let filename = format!("{name}.json");
        let json = screen_to_json(screen);
        fs::write(dir.join(&filename), &json).unwrap();
        println!("Generated {filename}");
    }
}
