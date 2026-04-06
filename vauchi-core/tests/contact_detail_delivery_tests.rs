// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for delivery status display in contact detail (J1 MVP).
//!
//! Users should see whether their card updates have been delivered
//! to a contact when viewing that contact's detail screen.

use vauchi_app::ui::contact_detail::{ContactDetailEngine, DeliverySummary};
use vauchi_app::ui::{Component, ContactItem, FieldDisplay, UiFieldVisibility, WorkflowEngine};

fn sample_contact() -> ContactItem {
    ContactItem {
        id: "c1".into(),
        name: "Bob".into(),
        subtitle: None,
        avatar_initials: "B".into(),
        status: None,
        searchable_fields: vec![],
    }
}

fn sample_fields() -> Vec<FieldDisplay> {
    vec![FieldDisplay {
        id: "f1".into(),
        field_type: "Phone".into(),
        label: "Mobile".into(),
        value: "+41-44-111-2233".into(),
        visibility: UiFieldVisibility::Shown,
    }]
}

// @scenario: update_propagation :: Contact detail shows delivery summary
#[test]
fn contact_detail_shows_delivery_summary_when_present() {
    let summary = DeliverySummary {
        total: 5,
        delivered: 3,
        pending: 1,
        failed: 1,
    };
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_delivery_summary(summary);

    let screen = engine.current_screen();
    let has_delivery = screen.components.iter().any(|c| match c {
        Component::InfoPanel { id, .. } => id == "delivery_status",
        _ => false,
    });
    assert!(
        has_delivery,
        "contact detail should show delivery_status InfoPanel"
    );
}

// @scenario: update_propagation :: All delivered shows success
#[test]
fn all_delivered_shows_success_message() {
    let summary = DeliverySummary {
        total: 3,
        delivered: 3,
        pending: 0,
        failed: 0,
    };
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_delivery_summary(summary);

    let screen = engine.current_screen();
    let delivery_panel = screen.components.iter().find_map(|c| match c {
        Component::InfoPanel { id, items, .. } if id == "delivery_status" => Some(items),
        _ => None,
    });
    assert!(delivery_panel.is_some(), "should have delivery panel");
    let items = delivery_panel.unwrap();
    let status_item = items.iter().find(|i| i.title == "Status");
    assert!(status_item.is_some(), "should have Status item");
    assert_eq!(status_item.unwrap().detail, "All delivered");
}

// @scenario: update_propagation :: No delivery records shows nothing
#[test]
fn no_delivery_summary_shows_no_delivery_section() {
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new());

    let screen = engine.current_screen();
    let has_delivery = screen.components.iter().any(|c| match c {
        Component::InfoPanel { id, .. } => id == "delivery_status",
        _ => false,
    });
    assert!(
        !has_delivery,
        "no delivery section when no summary provided"
    );
}

// @scenario: update_propagation :: Failed deliveries shown in summary
#[test]
fn failed_deliveries_shown_in_summary() {
    let summary = DeliverySummary {
        total: 4,
        delivered: 2,
        pending: 0,
        failed: 2,
    };
    let engine = ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
        .with_delivery_summary(summary);

    let screen = engine.current_screen();
    let delivery_panel = screen.components.iter().find_map(|c| match c {
        Component::InfoPanel { id, items, .. } if id == "delivery_status" => Some(items),
        _ => None,
    });
    let items = delivery_panel.expect("delivery panel should exist");
    let failed_item = items.iter().find(|i| i.title == "Failed");
    assert!(failed_item.is_some(), "should show failed count");
    assert_eq!(failed_item.unwrap().detail, "2");
}
