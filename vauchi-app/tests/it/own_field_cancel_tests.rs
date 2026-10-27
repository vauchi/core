// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Backing out of an edit or a removal of an own-card entry leaves the
//! card as it was, driven through the events the shells report.

use crate::event_path_support::{action, activate, first_input, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{ActionSpec, Command, Event, InputValue, PresentationNode, SurfaceId};

const MOBILE: &str = "+1-555-123-4567";

fn own_values(engine: &AppEngine) -> Vec<(String, String)> {
    engine
        .vauchi()
        .own_card()
        .unwrap()
        .unwrap()
        .fields()
        .iter()
        .map(|f| (f.label().to_string(), f.value().to_string()))
        .collect()
}

/// My Card with a "Mobile" phone entry, opened on that entry's detail.
fn mobile_entry_detail() -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(ContactField::new(FieldType::Phone, "Mobile", MOBILE, 0))
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    let (surface_id, entry) = row(&my_card, MOBILE);
    let detail = activate(&mut engine, surface_id, &entry);
    (engine, detail)
}

/// The surface's pending confirmation as (surface, confirm, cancel).
fn confirmation(commands: &[Command]) -> Option<(SurfaceId, ActionSpec, ActionSpec)> {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.into_iter().find_map(|node| match node {
                PresentationNode::Confirmation {
                    confirm, cancel, ..
                } => Some((surface_id.clone(), confirm, cancel)),
                _ => None,
            })
        })
}

fn has_input(commands: &[Command]) -> bool {
    surfaces(commands)
        .iter()
        .flat_map(|(_, nodes)| nodes)
        .any(|node| matches!(node, PresentationNode::Input { .. }))
}

// @scenario: contact_card_management :: Cancel editing preserves original values
#[test]
fn cancelling_an_edit_keeps_the_saved_value() {
    let (mut engine, detail) = mobile_entry_detail();
    let (surface_id, edit) = action(&detail, "Edit").expect("Edit");
    let form = activate(&mut engine, surface_id, &edit);
    let (surface_id, binding_id) = first_input(&form);
    engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("+1-555-999-8888".into()),
        })
        .unwrap();
    let (surface_id, actions) = action(&form, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &actions);
    let (surface_id, cancel) = action(&menu, "Cancel").expect("Cancel");
    let asked = activate(&mut engine, surface_id, &cancel);
    let (surface_id, discard, _) = confirmation(&asked).expect("a typed edit asks first");

    let after = activate(&mut engine, surface_id, &discard);

    assert!(!has_input(&after), "the form closed: {after:#?}");
    assert_eq!(
        own_values(&engine),
        vec![("Mobile".to_string(), MOBILE.to_string())]
    );
}

// @scenario: contact_card_management :: Cancel field removal
#[test]
fn cancelling_a_removal_keeps_the_entry() {
    let (mut engine, detail) = mobile_entry_detail();
    let (surface_id, delete) = row(&detail, "Delete");
    let asked = activate(&mut engine, surface_id, &delete);
    let (surface_id, _, cancel) = confirmation(&asked).expect("removal is confirmed first");

    let after = activate(&mut engine, surface_id, &cancel);

    assert_eq!(
        own_values(&engine),
        vec![("Mobile".to_string(), MOBILE.to_string())]
    );
    assert!(confirmation(&after).is_none(), "{after:#?}");
}
