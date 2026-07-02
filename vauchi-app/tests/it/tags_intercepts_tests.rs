// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the Tags management screen wiring (ADR-051,
//! Phase 4b): More-menu navigation and the delete intercept, driven
//! through a real in-memory `Vauchi`.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, Item, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;

fn tag_list_items(screen: &vauchi_app::ui::ScreenModel) -> Vec<Item> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "tags" => Some(items.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

fn screen_of(result: ActionResult) -> vauchi_app::ui::ScreenModel {
    match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected a screen-bearing result, got: {other:?}"),
    }
}

/// AppEngine with one contact that already carries the tag "climbing"
/// (seeded through the ContactDetail add-tag intercept).
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

// @internal
#[test]
fn more_menu_tags_entry_navigates_to_tags_screen() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::More);

    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "more_menu".into(),
        item_id: "tags".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen) => {
            assert_eq!(
                screen.screen_id, "tags",
                "the More → Tags entry must navigate to the Tags screen"
            );
        }
        other => panic!("expected NavigateTo(tags), got: {other:?}"),
    }
}

// @internal
#[test]
fn tags_screen_lists_the_seeded_tag_with_its_member_count() {
    let mut engine = engine_with_tagged_contact();
    let screen = engine.navigate_to(AppScreen::Tags);
    let rows = tag_list_items(&screen);
    assert_eq!(rows.len(), 1, "one tag in the vocabulary, got {rows:?}");
    assert_eq!(rows[0].name, "climbing");
    assert_eq!(
        rows[0].subtitle.as_deref(),
        Some("1 contact"),
        "member count reflects the one tagged contact"
    );
}

// @internal
#[test]
fn confirm_delete_tag_removes_it_via_intercept() {
    let mut engine = engine_with_tagged_contact();
    let screen = engine.navigate_to(AppScreen::Tags);
    let tag_id = tag_list_items(&screen)[0].id.clone();

    // Arm the per-row delete, then confirm — the AppEngine intercept calls
    // Vauchi::delete_tag and drops the row.
    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: tag_id,
        action_id: "request_delete".into(),
    });
    let after = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_tag".into(),
    }));
    assert!(
        tag_list_items(&after).is_empty(),
        "tag removed after confirm, got {:?}",
        tag_list_items(&after)
    );

    // Re-navigating must not resurrect it (no stale-cache / storage mismatch).
    let reloaded = engine.navigate_to(AppScreen::Tags);
    assert!(
        tag_list_items(&reloaded).is_empty(),
        "delete stays gone after re-navigation"
    );
}

// @internal
#[test]
fn cancel_delete_tag_keeps_the_tag() {
    let mut engine = engine_with_tagged_contact();
    let screen = engine.navigate_to(AppScreen::Tags);
    let tag_id = tag_list_items(&screen)[0].id.clone();

    let _ = engine.handle_action(UserAction::ListItemAction {
        component_id: "tags".into(),
        item_id: tag_id,
        action_id: "request_delete".into(),
    });
    let after = screen_of(engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_tag".into(),
    }));
    assert_eq!(
        tag_list_items(&after).len(),
        1,
        "cancel keeps the tag in the list"
    );
}

// A personal-note keystroke persists the note but must NOT rebuild the
// live ContactDetail engine — the in-progress add-tag query is transient
// engine state that a rebuild wipes mid-typing
// (2026-07-01-android-contacts-list-stale-after-mutation residual).
// @internal
#[test]
fn note_keystroke_preserves_in_progress_tag_query() {
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

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "add_tag".into(),
        value: "cli".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "personal_note".into(),
        value: "met at conf".into(),
    });

    let tag_query = engine.current_screen().components.iter().find_map(|c| {
        if let Component::TextInput { id, value, .. } = c
            && id == "add_tag"
        {
            Some(value.clone())
        } else {
            None
        }
    });
    assert_eq!(
        tag_query,
        Some("cli".to_string()),
        "the in-progress add-tag query must survive a note keystroke"
    );
    assert_eq!(
        engine.vauchi().load_personal_notes(&cid).unwrap(),
        Some(b"met at conf".to_vec()),
        "the note keystroke must still persist"
    );
}
