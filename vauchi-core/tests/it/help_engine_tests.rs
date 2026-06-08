// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn sample_items() -> Vec<HelpItem> {
    vec![
        HelpItem {
            id: "q1".into(),
            question: "How do I add a contact?".into(),
            answer: Some("Meet in person and use Exchange.".into()),
            answer_url: Some("https://docs.vauchi.app/users/faq#contacts--exchange".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "q2".into(),
            question: "What is end-to-end encryption?".into(),
            answer: None,
            answer_url: Some("https://docs.vauchi.app/users/faq#privacy--security".into()),
            category: "Security".into(),
        },
        HelpItem {
            id: "q3".into(),
            question: "How do I create a backup?".into(),
            answer: None,
            answer_url: None,
            category: "Getting Started".into(),
        },
    ]
}

// @scenario: help_faq :: View FAQ categories
// @internal
#[test]
fn help_engine_implements_workflow_engine() {
    let engine = HelpEngine::new(sample_items());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "help");
    assert_eq!(screen.title, "Help & FAQ");
    assert!(screen.progress.is_none());

    match &screen.components[0] {
        Component::TextInput { id, value, .. } => {
            assert_eq!(id, "help_search");
            assert_eq!(value, "");
        }
        other => panic!("Expected TextInput for search, got {:?}", other),
    }
}

// @scenario: help_faq :: Browse FAQs in a category
// @internal
#[test]
fn help_groups_by_category() {
    let engine = HelpEngine::new(sample_items());
    let screen = engine.current_screen();

    // 1 TextInput + 2 ActionList categories = 3 components
    assert_eq!(screen.components.len(), 3);

    match &screen.components[1] {
        Component::ActionList { id, items, .. } => {
            assert_eq!(id, "Getting Started");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, "q1");
            assert_eq!(items[1].id, "q3");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }

    match &screen.components[2] {
        Component::ActionList { id, items, .. } => {
            assert_eq!(id, "Security");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, "q2");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }
}

// @scenario: help_faq :: View a specific FAQ
// @internal
#[test]
fn help_faq_selected_shows_overlay() {
    let mut engine = HelpEngine::new(sample_items());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Getting Started".into(),
        item_id: "q1".into(),
    });

    match result {
        ActionResult::ShowInfoOverlay { title, body } => {
            assert_eq!(title, "How do I add a contact?");
            assert_eq!(body, "Meet in person and use Exchange.");
        }
        other => panic!("Expected ShowInfoOverlay, got {:?}", other),
    }
}

// @scenario: help_faq :: Related FAQs are linked
// @internal
#[test]
fn help_select_item_without_answer_falls_back_to_url() {
    let mut engine = HelpEngine::new(sample_items());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Security".into(),
        item_id: "q2".into(),
    });

    match result {
        ActionResult::OpenUrl { url } => {
            assert_eq!(url, "https://docs.vauchi.app/users/faq#privacy--security");
        }
        other => panic!("Expected OpenUrl, got {:?}", other),
    }
}

// @internal
#[test]
fn help_select_item_without_url_returns_update() {
    let mut engine = HelpEngine::new(sample_items());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Getting Started".into(),
        item_id: "q3".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "help");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @scenario: help_faq :: Search filters FAQs
// @internal
#[test]
fn help_engine_search_filters_faqs() {
    let mut engine = HelpEngine::new(sample_items());

    // Search for "encryption" — should match only q2
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "help_search".into(),
        value: "encryption".into(),
    });

    let screen = match result {
        ActionResult::UpdateScreen(s) => s,
        other => panic!("Expected UpdateScreen, got {:?}", other),
    };

    // TextInput + 1 ActionList (Security only)
    assert_eq!(screen.components.len(), 2);

    match &screen.components[0] {
        Component::TextInput { value, .. } => {
            assert_eq!(value, "encryption");
        }
        other => panic!("Expected TextInput, got {:?}", other),
    }

    match &screen.components[1] {
        Component::ActionList { id, items, .. } => {
            assert_eq!(id, "Security");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, "q2");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }
}

// @internal
#[test]
fn help_engine_search_matches_answer_text() {
    let mut engine = HelpEngine::new(sample_items());

    // Search for "exchange" — matches q1's answer text
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "help_search".into(),
        value: "exchange".into(),
    });
    let screen = engine.current_screen();

    // TextInput + 1 ActionList (Getting Started only, q1 matches)
    assert_eq!(screen.components.len(), 2);

    match &screen.components[1] {
        Component::ActionList { id, items, .. } => {
            assert_eq!(id, "Getting Started");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, "q1");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }
}

// @internal
#[test]
fn help_engine_search_is_case_insensitive() {
    let mut engine = HelpEngine::new(sample_items());

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "help_search".into(),
        value: "BACKUP".into(),
    });
    let screen = engine.current_screen();

    // TextInput + 1 ActionList (Getting Started, q3 matches)
    assert_eq!(screen.components.len(), 2);

    match &screen.components[1] {
        Component::ActionList { items, .. } => {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, "q3");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }
}

// @scenario: help_faq :: All categories have at least one FAQ
// @internal
#[test]
fn help_empty_items_shows_empty() {
    let engine = HelpEngine::new(vec![]);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "help");
    // Only the search TextInput, no ActionLists
    assert_eq!(screen.components.len(), 1);
    assert!(screen.actions.is_empty());
}
