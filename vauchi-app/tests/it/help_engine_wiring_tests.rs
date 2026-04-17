// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for Help screen wiring in AppEngine.
//!
//! Tests that tapping "Help Center" in Settings navigates to the Help screen,
//! and that the Help screen is reachable via direct navigation.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// ── Navigation ─────────────────────────────────────────────────

// @scenario: help_wiring :: Help is a valid AppScreen with roundtrip parsing
#[test]
fn help_screen_id_roundtrip() {
    let screen = AppScreen::Help;
    assert_eq!(screen.screen_id(), "help");
    assert_eq!(AppScreen::from_screen_id("help"), Some(AppScreen::Help));
}

// @scenario: help_wiring :: Navigate to Help shows the help screen
#[test]
fn navigate_to_help_shows_help_screen() {
    let mut engine = engine_with_identity();
    let screen = engine.navigate_to(AppScreen::Help);
    assert_eq!(screen.screen_id, "help");
    assert_eq!(screen.title, "Help & FAQ");
}

// @scenario: help_wiring :: Settings help_center navigates to Help screen
#[test]
fn settings_help_center_navigates_to_help_screen() {
    let mut engine = engine_with_identity();
    // Navigate to Settings first
    let _ = engine.navigate_to(AppScreen::Settings);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "help".into(),
        item_id: "help_center".into(),
    });

    match result {
        ActionResult::NavigateTo(screen) => {
            assert_eq!(screen.screen_id, "help", "Must navigate to help screen");
            assert_eq!(screen.title, "Help & FAQ");
        }
        other => panic!(
            "Expected NavigateTo with help screen, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// @scenario: help_wiring :: Help screen contains FAQ items after navigation from Settings
#[test]
fn help_screen_contains_faq_items_after_settings_navigation() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::Settings);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "help".into(),
        item_id: "help_center".into(),
    });

    if let ActionResult::NavigateTo(screen) = result {
        // Should have a search input + at least one ActionList category
        assert!(
            screen.components.len() >= 2,
            "Help screen must have search input + at least one FAQ category, got {} components",
            screen.components.len()
        );
    } else {
        panic!("Expected NavigateTo");
    }
}

// @scenario: help_wiring :: Help screen dispatches search action after navigation
#[test]
fn help_screen_dispatches_search_after_navigation() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::Settings);

    // Navigate to help
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "help".into(),
        item_id: "help_center".into(),
    });

    // Now the active screen should be Help — send a search action
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "help_search".into(),
        value: "backup".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "help", "Must remain on help screen");
            // The search should filter results — serialized screen should contain "backup"
            let json = serde_json::to_string(&screen).unwrap();
            assert!(
                json.to_lowercase().contains("backup"),
                "Filtered help screen must contain 'backup' items"
            );
        }
        other => panic!(
            "Expected UpdateScreen for search, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}

// @scenario: help_wiring :: Help screen dispatches FAQ selection after navigation
#[test]
fn help_screen_dispatches_faq_selection_after_navigation() {
    let mut engine = engine_with_identity();
    let _ = engine.navigate_to(AppScreen::Settings);

    // Navigate to help
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "help".into(),
        item_id: "help_center".into(),
    });

    // Select a FAQ item (the default items include "create-backup" with an inline answer)
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Getting Started".into(),
        item_id: "create-backup".into(),
    });

    match result {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "How do I create a backup?");
            assert!(!body.is_empty(), "FAQ answer must not be empty");
        }
        other => panic!(
            "Expected ShowInfoOverlay for FAQ item, got {:?}",
            std::mem::discriminant(&other)
        ),
    }
}
