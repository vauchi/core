// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;

fn make_item(id: &str, name: &str, status: Status, retryable: bool) -> DeliveryItem {
    DeliveryItem {
        contact_id: id.to_string(),
        contact_name: name.to_string(),
        status,
        detail: None,
        retryable,
    }
}

// @internal
#[test]
fn delivery_screen_id() {
    let engine = DeliveryStatusEngine::new(vec![]);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "delivery_status");
}

// @internal
#[test]
fn delivery_empty_shows_all_delivered() {
    let engine = DeliveryStatusEngine::new(vec![]);
    let screen = engine.current_screen();

    assert_eq!(screen.components.len(), 1);
    match &screen.components[0] {
        Component::InfoPanel {
            id,
            icon,
            title,
            items,
            ..
        } => {
            assert_eq!(id, "empty");
            assert_eq!(icon.as_deref(), Some("checkmark"));
            assert_eq!(title, "All Delivered");
            assert!(items.is_empty());
        }
        other => panic!("expected InfoPanel, got {:?}", other),
    }
}

// @internal
#[test]
fn delivery_shows_status_per_item() {
    let items = vec![
        make_item("c1", "Alice", Status::Success, false),
        make_item("c2", "Bob", Status::Failed, true),
    ];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    assert_eq!(screen.components.len(), 2);
    match &screen.components[0] {
        Component::StatusIndicator { id, title, .. } => {
            assert_eq!(id, "c1");
            assert_eq!(title, "Alice");
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
    match &screen.components[1] {
        Component::StatusIndicator { id, title, .. } => {
            assert_eq!(id, "c2");
            assert_eq!(title, "Bob");
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

// @internal
#[test]
fn delivery_select_contact_opens_it() {
    let items = vec![make_item("c1", "Alice", Status::Success, false)];
    let mut engine = DeliveryStatusEngine::new(items);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "status_list".to_string(),
        item_id: "c1".to_string(),
    });

    match result {
        ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "c1"),
        other => panic!("expected OpenContact, got {:?}", other),
    }
}

// @internal
#[test]
fn delivery_retry_button_when_retryable() {
    let items = vec![
        make_item("c1", "Alice", Status::Success, false),
        make_item("c2", "Bob", Status::Failed, true),
    ];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    assert_eq!(screen.actions.len(), 1);
    assert_eq!(screen.actions[0].id, "retry_all");
    assert_eq!(screen.actions[0].label, "Retry Failed");
    assert_eq!(screen.actions[0].style, ActionStyle::Primary);
    assert!(screen.actions[0].enabled);
}

// @internal
#[test]
fn delivery_no_retry_when_all_success() {
    let items = vec![
        make_item("c1", "Alice", Status::Success, false),
        make_item("c2", "Bob", Status::Success, false),
    ];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    assert!(screen.actions.is_empty());
}

// @internal
#[test]
fn delivery_mixed_statuses() {
    let items = vec![
        make_item("c1", "Alice", Status::Success, false),
        make_item("c2", "Bob", Status::Failed, true),
        make_item("c3", "Carol", Status::Pending, false),
        make_item("c4", "Dave", Status::InProgress, false),
        make_item("c5", "Eve", Status::Warning, false),
    ];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    assert_eq!(screen.components.len(), 5);

    let expected = [
        ("c1", Status::Success),
        ("c2", Status::Failed),
        ("c3", Status::Pending),
        ("c4", Status::InProgress),
        ("c5", Status::Warning),
    ];

    for (component, (expected_id, expected_status)) in screen.components.iter().zip(expected.iter())
    {
        match component {
            Component::StatusIndicator { id, status, .. } => {
                assert_eq!(id, expected_id);
                assert_eq!(status, expected_status);
            }
            other => panic!("expected StatusIndicator, got {:?}", other),
        }
    }
}
