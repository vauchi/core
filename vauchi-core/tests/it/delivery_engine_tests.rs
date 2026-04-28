// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cross-crate integration tests for `DeliveryStatusEngine`.
//!
//! Engine-internal unit tests live in `vauchi-app/src/ui/delivery.rs`'s
//! `mod tests`. The tests here verify that the engine remains usable
//! through the public re-exports from `vauchi_app::ui::*` (the surface
//! consumers like the platform layer see) and capture cross-section
//! integration cases that are hard to cover from inside the crate.

use vauchi_app::ui::*;

fn make_item(
    message: &str,
    contact: &str,
    name: &str,
    status: Status,
    retryable: bool,
) -> DeliveryItem {
    DeliveryItem {
        message_id: message.to_string(),
        contact_id: contact.to_string(),
        contact_name: name.to_string(),
        status,
        detail: None,
        retryable,
    }
}

// @internal
#[test]
fn delivery_screen_id_and_title() {
    let engine = DeliveryStatusEngine::new(vec![]);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "delivery_status");
    assert_eq!(screen.title, "Delivery Status");
}

// @internal
#[test]
fn delivery_empty_shows_all_delivered_panel() {
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
fn delivery_shows_recent_section_for_delivered_items() {
    let items = vec![make_item("m1", "c1", "Alice", Status::Success, false)];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    // Section header + 1 indicator
    assert_eq!(screen.components.len(), 2);
    match &screen.components[0] {
        Component::Text { content, .. } => assert_eq!(content, "Recent"),
        other => panic!("expected Text section header, got {:?}", other),
    }
    match &screen.components[1] {
        Component::StatusIndicator {
            id, title, status, ..
        } => {
            assert_eq!(id, "m1");
            assert_eq!(title, "Alice");
            assert_eq!(*status, Status::Success);
        }
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}

// @internal
#[test]
fn delivery_select_routes_to_open_contact() {
    let items = vec![make_item("m1", "c1", "Alice", Status::Success, false)];
    let mut engine = DeliveryStatusEngine::new(items);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "section_recent".into(),
        item_id: "c1".into(),
    });

    match result {
        ActionResult::OpenContact { contact_id } => assert_eq!(contact_id, "c1"),
        other => panic!("expected OpenContact, got {:?}", other),
    }
}

// @internal
#[test]
fn delivery_failed_section_emits_retry_all_action() {
    let items = vec![
        make_item("m1", "c1", "Alice", Status::Success, false),
        make_item("m2", "c2", "Bob", Status::Failed, true),
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
        make_item("m1", "c1", "Alice", Status::Success, false),
        make_item("m2", "c2", "Bob", Status::Success, false),
    ];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    assert!(screen.actions.is_empty());
}

// @internal
#[test]
fn delivery_failed_row_id_is_message_id() {
    let items = vec![make_item("msg-bob", "c-bob", "Bob", Status::Failed, true)];
    let engine = DeliveryStatusEngine::new(items);
    let screen = engine.current_screen();

    // Header + 1 failed indicator
    assert_eq!(screen.components.len(), 2);
    match &screen.components[1] {
        Component::StatusIndicator { id, .. } => assert_eq!(id, "msg-bob"),
        other => panic!("expected StatusIndicator, got {:?}", other),
    }
}
