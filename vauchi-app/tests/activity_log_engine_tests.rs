// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the activity log workflow engine.
//!
//! Verifies empty-state rendering and list rendering from `ActivityLogItem`
//! entries without any storage dependency.

use vauchi_app::notification_types::{ActivityLogEntry, EventOrigin};
use vauchi_app::ui::{
    ActionResult, ActivityLogEngine, ActivityLogItem, Component, UserAction, WorkflowEngine,
};

// @scenario: activity_log.feature - Empty log shows empty state message
// @internal
#[test]
fn empty_log_shows_empty_state() {
    let engine = ActivityLogEngine::new(vec![]);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "activity_log");
    assert_eq!(screen.title, "Activity");
    assert_eq!(
        screen.components.len(),
        1,
        "Empty log must render exactly one component"
    );
    let screen_json = serde_json::to_string(&screen).unwrap();
    assert!(
        screen_json.contains("No recent activity"),
        "Empty state must contain 'No recent activity'"
    );
}

// @scenario: activity_log.feature - Entries render as ActionList
// @internal
#[test]
fn entries_render_as_list_items() {
    let items = vec![
        ActivityLogItem {
            event_key: "evt-001".into(),
            entry: ActivityLogEntry::CardUpdateReceived {
                contact_id: "contact-abc".into(),
                changed_fields: vec!["phone".into()],
            },
            contact_name: "Alice".into(),
            created_at: 1_700_000_000,
        },
        ActivityLogItem {
            event_key: "evt-002".into(),
            entry: ActivityLogEntry::ContactAdded {
                contact_id: "contact-def".into(),
                origin: EventOrigin::Local,
            },
            contact_name: "Bob".into(),
            created_at: 1_700_000_100,
        },
    ];

    let engine = ActivityLogEngine::new(items);
    let screen = engine.current_screen();

    assert_eq!(screen.screen_id, "activity_log");
    assert_eq!(
        screen.components.len(),
        1,
        "Non-empty log must render exactly one component"
    );

    let component = &screen.components[0];
    assert!(
        matches!(component, Component::ActionList { .. }),
        "Non-empty log must render an ActionList, got: {:?}",
        component
    );

    if let Component::ActionList { id, items } = component {
        assert_eq!(id, "activity_list");
        assert_eq!(items.len(), 2, "ActionList must contain all provided items");
        assert_eq!(items[0].id, "evt-001");
        assert_eq!(items[1].id, "evt-002");
        assert!(
            items[0].label.contains("Alice"),
            "First item label must reference contact name 'Alice'"
        );
        assert!(
            items[1].label.contains("Bob"),
            "Second item label must reference contact name 'Bob'"
        );
    }
}

// @scenario: activity_log.feature - Selecting a list item opens the contact
// @internal
#[test]
fn list_item_selected_opens_contact() {
    let items = vec![ActivityLogItem {
        event_key: "evt-abc".into(),
        entry: ActivityLogEntry::EmergencyAlertReceived {
            contact_id: "contact-xyz".into(),
        },
        contact_name: "Carol".into(),
        created_at: 1_700_000_200,
    }];

    let mut engine = ActivityLogEngine::new(items);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "activity_list".into(),
        item_id: "evt-abc".into(),
    });

    assert!(
        matches!(
            &result,
            ActionResult::OpenContact { contact_id } if contact_id == "contact-xyz"
        ),
        "Selecting a log item must open the associated contact, got: {:?}",
        result
    );
}
