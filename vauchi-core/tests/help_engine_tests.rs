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
            answer_url: Some("https://docs.vauchi.app/faq/add-contact".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "q2".into(),
            question: "What is end-to-end encryption?".into(),
            answer: None,
            answer_url: Some("https://docs.vauchi.app/faq/e2e".into()),
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
#[test]
fn help_screen_id() {
    let engine = HelpEngine::new(sample_items());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "help");
    assert_eq!(screen.title, "Help & FAQ");
    assert!(screen.progress.is_none());
}

// @scenario: help_faq :: Browse FAQs in a category
#[test]
fn help_groups_by_category() {
    let engine = HelpEngine::new(sample_items());
    let screen = engine.current_screen();

    // 2 unique categories → 2 ActionList components
    assert_eq!(screen.components.len(), 2);

    match &screen.components[0] {
        Component::ActionList { id, items } => {
            assert_eq!(id, "Getting Started");
            assert_eq!(items.len(), 2);
            assert_eq!(items[0].id, "q1");
            assert_eq!(items[1].id, "q3");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }

    match &screen.components[1] {
        Component::ActionList { id, items } => {
            assert_eq!(id, "Security");
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].id, "q2");
        }
        other => panic!("Expected ActionList, got {:?}", other),
    }
}

// @scenario: help_faq :: View a specific FAQ
#[test]
fn help_select_item_with_answer_shows_inline_alert() {
    let mut engine = HelpEngine::new(sample_items());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Getting Started".into(),
        item_id: "q1".into(),
    });

    match result {
        ActionResult::ShowAlert { title, message } => {
            assert_eq!(title, "How do I add a contact?");
            assert_eq!(message, "Meet in person and use Exchange.");
        }
        other => panic!("Expected ShowAlert, got {:?}", other),
    }
}

// @scenario: help_faq :: Related FAQs are linked
#[test]
fn help_select_item_without_answer_falls_back_to_url() {
    let mut engine = HelpEngine::new(sample_items());
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "Security".into(),
        item_id: "q2".into(),
    });

    match result {
        ActionResult::OpenUrl { url } => {
            assert_eq!(url, "https://docs.vauchi.app/faq/e2e");
        }
        other => panic!("Expected OpenUrl, got {:?}", other),
    }
}

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

// @scenario: help_faq :: All categories have at least one FAQ
#[test]
fn help_empty_items_shows_empty() {
    let engine = HelpEngine::new(vec![]);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "help");
    assert!(screen.components.is_empty());
    assert!(screen.actions.is_empty());
}
