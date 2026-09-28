// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deleting an own-card entry (#427, owner decisions 2026-09-28): Delete
//! sits in the screen body as a destructive row beside the entry, not in
//! the Actions menu, and asks for confirmation naming the entry before
//! anything is removed. Undo stays available after confirming.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, SettingsItemKind, UserAction,
    WorkflowEngine,
};
use vauchi_core::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};

const NOW: u64 = 1_790_000_000;

fn open_entry() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity("Ada").expect("identity");
    let field = ContactField::new(FieldType::Phone, "mobile", "+41 79 000 00 00", NOW);
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).expect("own field");
    let mut engine = AppEngine::new(vauchi);
    let _ = engine.navigate_to(AppScreen::MyInfoEntryDetail {
        field_id: field_id.clone(),
    });
    (engine, field_id)
}

fn delete_row(screen: &ScreenModel) -> Option<(String, String, Option<String>)> {
    screen.components.iter().find_map(|c| match c {
        Component::SettingsGroup { id, items, .. } if id == "entry_actions" => {
            items.iter().find(|i| i.id == "delete").map(|i| {
                assert!(
                    matches!(i.kind, SettingsItemKind::Destructive { .. }),
                    "Delete must be a destructive row"
                );
                (
                    id.clone(),
                    i.label.clone(),
                    i.a11y.as_ref().and_then(|a| a.label.clone()),
                )
            })
        }
        _ => None,
    })
}

fn confirm_warning(screen: &ScreenModel) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm {
            warning,
            destructive,
            ..
        } => {
            assert!(*destructive, "the confirm action must read as destructive");
            Some(warning.clone())
        }
        _ => None,
    })
}

fn entry_exists(engine: &AppEngine, field_id: &str) -> bool {
    engine
        .vauchi()
        .own_card()
        .expect("own card")
        .expect("card")
        .fields()
        .iter()
        .any(|f| f.id() == field_id)
}

fn press(engine: &mut AppEngine, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: id.into(),
    })
}

fn select_delete(engine: &mut AppEngine) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: "entry_actions".into(),
        item_id: "delete".into(),
    })
}

// @internal
#[test]
fn delete_is_a_labelled_destructive_row_in_the_screen_body() {
    let (engine, _) = open_entry();
    let screen = engine.current_screen();

    let (_, label, a11y) = delete_row(&screen).expect("a Delete row in the body");
    assert_eq!(label, "Delete");
    assert_eq!(a11y.as_deref(), Some("Delete mobile"));
    assert!(
        !screen.contextual_actions.iter().any(|a| a.id == "delete"),
        "Delete must not also hide in the Actions menu"
    );
}

// @internal
#[test]
fn choosing_delete_asks_first_and_names_the_entry() {
    let (mut engine, field_id) = open_entry();

    let _ = select_delete(&mut engine);
    let screen = engine.current_screen();

    assert_eq!(
        confirm_warning(&screen).as_deref(),
        Some("Delete mobile? Contacts who can see it lose it on their next sync.")
    );
    assert!(
        entry_exists(&engine, &field_id),
        "nothing is deleted before confirming"
    );
}

// @internal
#[test]
fn cancelling_keeps_the_entry_and_closes_the_question() {
    let (mut engine, field_id) = open_entry();
    let _ = select_delete(&mut engine);

    let _ = press(&mut engine, "cancel_delete_entry");

    assert!(confirm_warning(&engine.current_screen()).is_none());
    assert!(entry_exists(&engine, &field_id));
}

// @internal
#[test]
fn confirming_deletes_the_entry_and_offers_undo() {
    let (mut engine, field_id) = open_entry();
    let _ = select_delete(&mut engine);

    let result = press(&mut engine, "confirm_delete_entry");

    assert!(!entry_exists(&engine, &field_id));
    match result {
        ActionResult::ShowToast { undo_action_id, .. } => assert_eq!(
            undo_action_id.as_deref(),
            Some(format!("undo_delete_field:{field_id}").as_str())
        ),
        other => panic!("expected the Undo toast, got {other:?}"),
    }
}
