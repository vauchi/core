// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The contacts screen as the shells receive it: its empty state, and
//! backing out of deleting a contact.

use crate::event_path_support::{action, activate, confirmation, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::ImportSource;
use vauchi_core::{Command, Contact, ContactCard, PresentationNode};

fn contacts_screen(vauchi: Vauchi) -> (AppEngine, Vec<Command>) {
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    let commands = engine.initial_commands().unwrap();
    (engine, commands)
}

fn group_labels(commands: &[Command]) -> Vec<String> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::Group { label, .. } => label,
            _ => None,
        })
        .collect()
}

fn contact_names(engine: &AppEngine) -> Vec<String> {
    engine
        .vauchi()
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.display_name().to_string())
        .collect()
}

// @scenario: contacts_management :: Empty contacts list
#[test]
fn with_no_contacts_the_screen_says_so() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let (engine, commands) = contacts_screen(vauchi);

    assert_eq!(contact_names(&engine), Vec::<String>::new());
    assert!(
        group_labels(&commands).contains(&"No contacts yet".to_string()),
        "{commands:#?}"
    );
}

/// Only imported contacts can be deleted; exchanged ones are archived.
// @scenario: contacts_management :: Cancel contact removal
#[test]
fn cancelling_a_contact_deletion_keeps_the_contact() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_contact(Contact::from_import(
            "contact-dave".into(),
            ContactCard::new("Dave"),
            ImportSource::VcardFile,
            None,
            0,
        ))
        .unwrap();
    let (mut engine, list) = contacts_screen(vauchi);
    let (surface_id, dave) = row(&list, "Dave");
    let detail = activate(&mut engine, surface_id, &dave);
    let (surface_id, actions) = action(&detail, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &actions);
    let (surface_id, delete) = action(&menu, "Delete Contact").expect("Delete Contact");
    let asked = activate(&mut engine, surface_id, &delete);
    let (surface_id, _, cancel) = confirmation(&asked).expect("deletion is confirmed first");

    let after = activate(&mut engine, surface_id, &cancel);

    assert_eq!(contact_names(&engine), ["Dave"]);
    assert!(confirmation(&after).is_none(), "{after:#?}");
}
