// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A private note on a field a contact shared, written on their screen the
//! way the shells drive it.

use crate::event_path_support::{activate, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{
    ActionSpec, BindingId, Command, Contact, ContactCard, Event, Identity, InputValue,
    PresentationNode, SurfaceId, SymmetricKey,
};

const FIELD_NOTE: &str = "Private note for this field";

/// Alice's app open on the screen of Bob, who shared a "Mobile" phone.
fn bobs_screen() -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let peer = Identity::create("Bob", 0);
    let mut card = ContactCard::new("Bob");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Mobile",
        "+1-555-000-1111",
        0,
    ))
    .unwrap();
    let bob = Contact::from_exchange(
        *peer.signing_public_key(),
        card,
        SymmetricKey::generate(),
        0,
    );
    vauchi.add_contact(bob).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    let list = engine.initial_commands().unwrap();
    let (surface_id, bob) = row(&list, "Bob");
    let screen = activate(&mut engine, surface_id, &bob);
    (engine, screen)
}

/// The children of the field-note group, with the surface showing it.
fn field_note(commands: &[Command]) -> (SurfaceId, Vec<PresentationNode>) {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.into_iter().find_map(|node| match node {
                PresentationNode::Group {
                    label, children, ..
                } if label.as_deref() == Some(FIELD_NOTE) => Some((surface_id.clone(), children)),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("no field note: {commands:#?}"))
}

fn note_action(children: &[PresentationNode], title_wanted: &str) -> ActionSpec {
    children
        .iter()
        .find_map(|node| match node {
            PresentationNode::Status {
                title, activation, ..
            } if title == title_wanted => activation.clone(),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no {title_wanted} in {children:#?}"))
}

fn note_input(children: &[PresentationNode]) -> BindingId {
    children
        .iter()
        .find_map(|node| match node {
            PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no note input in {children:#?}"))
}

// @scenario: navigation :: Add private note to a contact's shared field
#[test]
fn a_note_written_on_a_shared_field_is_saved() {
    let (mut engine, screen) = bobs_screen();
    let (surface_id, children) = field_note(&screen);
    let editing = activate(&mut engine, surface_id, &note_action(&children, "Edit"));
    let (surface_id, children) = field_note(&editing);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id: surface_id.clone(),
            binding_id: note_input(&children),
            value: InputValue::Text("Call before 5pm".into()),
        })
        .unwrap();
    let save = note_action(&field_note(&typed).1, "Save");
    activate(&mut engine, surface_id, &save);

    let bob_id = engine.vauchi().list_contacts().unwrap()[0].id().to_string();
    let saved: Vec<String> = engine
        .vauchi()
        .load_contact_field_notes(&bob_id)
        .unwrap()
        .into_values()
        .map(|note| String::from_utf8(note).unwrap())
        .collect();
    assert_eq!(saved, ["Call before 5pm"]);
}
