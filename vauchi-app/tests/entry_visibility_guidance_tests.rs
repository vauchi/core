// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use vauchi_app::ui::{
    ActionResult, Component, MyInfoEngine, MyInfoProgress, OwnFieldInfo, UserAction, WorkflowEngine,
};

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn card_explains_visibility_defaults_and_how_to_review_the_audience() {
    for (shown, groups, expected) in [
        (false, vec![], "Hidden by default"),
        (true, vec![], "All contacts by default"),
        (false, vec!["Family".to_owned()], "Family"),
    ] {
        let mut engine = MyInfoEngine::new(MyInfoProgress::default()).with_own_card(
            "Ada".into(),
            vec![OwnFieldInfo {
                field_id: "email".into(),
                field_type: "Email".into(),
                label: "Work".into(),
                value: "ada@example.invalid".into(),
                visible_groups: groups,
                contact_count: 0,
                shown,
            }],
        );
        let screen = engine.current_screen();
        let row = screen
            .components
            .iter()
            .find_map(|component| match component {
                Component::List { id, items, .. } if id == "own_entries" => items.first(),
                _ => None,
            })
            .expect("entry row");
        assert_eq!(row.status.as_deref(), Some(expected));
        assert!(screen.components.iter().any(|component| matches!(component,
            Component::Text { content, .. }
                if content == "Open an entry to review who can see it."
        )));
        assert!(matches!(engine.handle_action(UserAction::ListItemSelected {
            component_id: "own_entries".into(), item_id: row.id.clone(),
        }), ActionResult::OpenEntryDetail { field_id } if field_id == "email"));
    }
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn empty_card_does_not_ask_to_open_a_missing_entry() {
    let screen = MyInfoEngine::new(MyInfoProgress::default()).current_screen();
    assert_eq!(
        screen
            .components
            .iter()
            .filter(|component| matches!(component,
                Component::Text { content, .. }
                    if content == "Open an entry to review who can see it."
            ))
            .count(),
        0
    );
}
