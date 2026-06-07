// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-level tests for `TagsEngine` (ADR-051 contact annotations,
//! Phase 4b — tag management list).
//!
//! Rendering + the delete-confirmation state machine only. The actual
//! `Vauchi::delete_tag` call flows through the AppEngine intercept and is
//! covered in `tags_intercepts_tests.rs`.

use vauchi_app::ui::{Component, TagSummary, TagsEngine, UserAction, WorkflowEngine};

fn sample_tags() -> Vec<TagSummary> {
    vec![
        TagSummary {
            id: "t1".into(),
            name: "climbing".into(),
            member_count: 3,
        },
        TagSummary {
            id: "t2".into(),
            name: "work".into(),
            member_count: 1,
        },
    ]
}

fn tag_list_items(screen: &vauchi_app::ui::ScreenModel) -> Vec<vauchi_app::ui::Item> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "tags" => Some(items.clone()),
            _ => None,
        })
        .expect("tags List component must be present")
}

fn has_delete_confirm(screen: &vauchi_app::ui::ScreenModel) -> bool {
    screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_tag" && *destructive)
    })
}

// @internal
#[test]
fn renders_tags_with_member_counts_and_delete_action() {
    let screen = TagsEngine::new(sample_tags()).current_screen();
    assert_eq!(screen.screen_id, "tags");
    assert_eq!(screen.title, "Tags");

    let items = tag_list_items(&screen);
    assert_eq!(
        items.iter().map(|i| i.name.as_str()).collect::<Vec<_>>(),
        vec!["climbing", "work"]
    );
    assert_eq!(items[0].subtitle.as_deref(), Some("3 contacts"));
    assert_eq!(
        items[1].subtitle.as_deref(),
        Some("1 contact"),
        "singular member count"
    );

    // Each row carries exactly one per-row delete affordance.
    for item in &items {
        let ids: Vec<&str> = item.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec!["request_delete"], "row {} actions", item.id);
    }
}

// @internal
#[test]
fn empty_vocabulary_renders_empty_list_no_confirm() {
    let screen = TagsEngine::new(vec![]).current_screen();
    assert!(tag_list_items(&screen).is_empty());
    assert!(
        !has_delete_confirm(&screen),
        "no confirm without a pending delete"
    );
}

// @internal
#[test]
fn request_delete_arms_inline_confirm_naming_the_tag() {
    let mut engine = TagsEngine::new(sample_tags());
    assert!(!has_delete_confirm(&engine.current_screen()));

    let result = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: "t1".into(),
        action_id: "request_delete".into(),
    });
    assert!(matches!(
        result,
        vauchi_app::ui::ActionResult::UpdateScreen(_)
    ));

    assert_eq!(engine.pending_delete_id(), Some("t1"));
    let screen = engine.current_screen();
    assert!(has_delete_confirm(&screen), "InlineConfirm must appear");
    let warning = screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm { id, warning, .. } if id == "delete_tag" => Some(warning.clone()),
        _ => None,
    });
    assert!(
        warning.unwrap().contains("climbing"),
        "confirmation names the target tag"
    );
}

// @internal
#[test]
fn cancel_delete_clears_the_pending_confirmation() {
    let mut engine = TagsEngine::new(sample_tags());
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: "t1".into(),
        action_id: "request_delete".into(),
    });
    assert_eq!(engine.pending_delete_id(), Some("t1"));

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_tag".into(),
    });
    assert_eq!(engine.pending_delete_id(), None);
    assert!(!has_delete_confirm(&engine.current_screen()));
}

// @internal
#[test]
fn confirm_delete_drops_the_row_and_clears_pending() {
    // Models what the AppEngine intercept does after Vauchi::delete_tag.
    let mut engine = TagsEngine::new(sample_tags());
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: "t1".into(),
        action_id: "request_delete".into(),
    });

    engine.confirm_delete();

    assert_eq!(engine.pending_delete_id(), None);
    let names: Vec<String> = tag_list_items(&engine.current_screen())
        .iter()
        .map(|i| i.name.clone())
        .collect();
    assert_eq!(names, vec!["work"], "deleted tag is gone, others remain");
}
