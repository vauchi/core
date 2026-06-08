// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the tag→group promotion flow (ADR-051, Phase 4b):
//! `TagPromotionEngine` rendering + the AppEngine intercept that opens the
//! review from the Tags screen and confirms it through a real `Vauchi`.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, PromotionField, TagPromotionEngine, UserAction,
    WorkflowEngine,
};
use vauchi_core::api::Vauchi;

// ── Engine-level ───────────────────────────────────────────────────────────

fn engine() -> TagPromotionEngine {
    TagPromotionEngine::new(
        "t1".into(),
        "climbing".into(),
        2,
        vec![
            PromotionField {
                field_id: "f1".into(),
                label: "Email".into(),
                value: "a@b.c".into(),
                selected: true,
            },
            PromotionField {
                field_id: "f2".into(),
                label: "Phone".into(),
                value: "123".into(),
                selected: false,
            },
        ],
    )
}

fn toggle_items(screen: &vauchi_app::ui::ScreenModel) -> Vec<(String, bool)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList { id, items, .. } if id == "promotion_fields" => Some(items),
            _ => None,
        })
        .map(|items| items.iter().map(|i| (i.id.clone(), i.selected)).collect())
        .unwrap_or_default()
}

// @internal
#[test]
fn renders_draft_with_confirm_and_field_toggles() {
    let screen = engine().current_screen();
    assert_eq!(screen.screen_id, "tag_promotion");
    let action_ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(action_ids, vec!["confirm_promotion"]);

    assert_eq!(
        toggle_items(&screen),
        vec![("f1".to_string(), true), ("f2".to_string(), false)],
        "draft pre-selects the inherited visible fields"
    );
    assert_eq!(engine().selected_field_ids(), vec!["f1".to_string()]);
}

// @internal
#[test]
fn toggling_a_field_flips_its_selection() {
    let mut e = engine();
    let _ = e.handle_action(UserAction::ItemToggled {
        component_id: "promotion_fields".into(),
        item_id: "f2".into(),
    });
    assert_eq!(
        e.selected_field_ids(),
        vec!["f1".to_string(), "f2".to_string()]
    );

    let _ = e.handle_action(UserAction::ItemToggled {
        component_id: "promotion_fields".into(),
        item_id: "f1".into(),
    });
    assert_eq!(e.selected_field_ids(), vec!["f2".to_string()]);
}

// ── Intercept / integration ────────────────────────────────────────────────

/// AppEngine with own card + a contact tagged "climbing".
fn engine_with_tagged_contact() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .import_contacts_from_vcf(b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n")
        .unwrap();
    let cid = vauchi.list_contacts().unwrap()[0].id().to_string();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: cid.clone(),
    });
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "add_tag:climbing".into(),
    });
    engine
}

fn tag_row_id(engine: &mut AppEngine) -> String {
    let screen = engine.navigate_to(AppScreen::Tags);
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "tags" => Some(items[0].id.clone()),
            _ => None,
        })
        .expect("a tag row")
}

// @internal
#[test]
fn promote_row_action_opens_the_promotion_review() {
    let mut engine = engine_with_tagged_contact();
    let tag_id = tag_row_id(&mut engine);

    let result = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: tag_id,
        action_id: "promote".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(screen.screen_id, "tag_promotion");
            assert!(
                screen.components.iter().any(|c| matches!(
                    c,
                    Component::InfoPanel { id, .. } if id == "promotion_info"
                )),
                "review screen shows the draft info"
            );
        }
        other => panic!("promote must navigate to tag_promotion, got: {other:?}"),
    }
}

// @internal
#[test]
fn confirm_promotion_creates_group_and_consumes_tag() {
    let mut engine = engine_with_tagged_contact();
    let tag_id = tag_row_id(&mut engine);
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: tag_id,
        action_id: "promote".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_promotion".into(),
    });

    // Lands on the Groups list, which now contains the promoted group.
    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(screen.screen_id, "groups");
            let group_names: Vec<String> = screen
                .components
                .iter()
                .find_map(|c| match c {
                    Component::ActionList { id, items } if id == "groups" => Some(items),
                    _ => None,
                })
                .map(|items| items.iter().map(|i| i.label.clone()).collect())
                .unwrap_or_default();
            assert!(
                group_names.contains(&"climbing".to_string()),
                "promoted group present, got {group_names:?}"
            );
        }
        other => panic!("confirm must navigate to groups, got: {other:?}"),
    }

    // The tag is consumed (replace semantics) — Tags list is now empty.
    let tags_screen = engine.navigate_to(AppScreen::Tags);
    let remaining = tags_screen.components.iter().find_map(|c| match c {
        Component::List { id, items, .. } if id == "tags" => Some(items.len()),
        _ => None,
    });
    assert_eq!(remaining, Some(0), "promoted tag is deleted");
}
