// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The private note on an own-card entry, seen and changed in the entry's
//! edit form the way the shells drive it.

use crate::event_path_support::{action, activate, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{BindingId, Command, Event, InputValue, PresentationNode, SurfaceId};

const WORK: &str = "alice@work.example";
const NOTE: &str = "Check spam folder weekly";

/// The edit form of Alice's "Work" email, which carries `NOTE`.
fn work_email_edit_form() -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(
            ContactField::new(FieldType::Email, "Work", WORK, 0).with_note(NOTE.to_string()),
        )
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    let (surface_id, entry) = row(&my_card, WORK);
    let detail = activate(&mut engine, surface_id, &entry);
    let (surface_id, edit) = action(&detail, "Edit").expect("Edit");
    let form = activate(&mut engine, surface_id, &edit);
    (engine, form)
}

/// The form input currently holding `value`.
fn input_holding(commands: &[Command], value: &str) -> Option<(SurfaceId, BindingId)> {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.into_iter().find_map(|node| match node {
                PresentationNode::Input {
                    binding_id,
                    value: shown,
                    ..
                } if shown == value => Some((surface_id.clone(), binding_id)),
                _ => None,
            })
        })
}

fn work_note(engine: &AppEngine) -> Option<String> {
    engine
        .vauchi()
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .find(|f| f.label() == "Work")
        .and_then(|f| f.note().map(str::to_string))
}

// @scenario: navigation :: View and edit private note on own field
#[test]
fn the_edit_form_shows_the_note_and_saves_a_change() {
    let (mut engine, form) = work_email_edit_form();
    let (surface_id, binding_id) =
        input_holding(&form, NOTE).expect("the edit form shows the saved note");

    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("Forward to personal inbox".into()),
        })
        .unwrap();
    let (surface_id, save) = action(&typed, "Save")
        .or_else(|| action(&form, "Save"))
        .expect("Save");
    activate(&mut engine, surface_id, &save);

    assert_eq!(
        work_note(&engine).as_deref(),
        Some("Forward to personal inbox")
    );
}
