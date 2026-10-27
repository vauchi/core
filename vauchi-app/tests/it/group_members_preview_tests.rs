// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A group's screen as the shells receive it: who is in it, how much of
//! my card it sees, and previewing my card as one of its members.

use crate::event_path_support::{action, activate, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{Command, Contact, ContactCard, Identity, PresentationNode, SymmetricKey};

/// Alice's "Family" group holds `members` and sees her one "Work" email.
fn family_screen(members: &[&str]) -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let work = ContactField::new(FieldType::Email, "Work", "alice@example.org", 0);
    let work_id = work.id().to_string();
    vauchi.add_own_field(work).unwrap();
    let family = vauchi.create_group("Family").unwrap();
    vauchi
        .set_group_field_visibility(family.id(), &work_id, true)
        .unwrap();
    for name in members {
        let peer = Identity::create(name, 0);
        let contact = Contact::from_exchange(
            *peer.signing_public_key(),
            ContactCard::new(name),
            SymmetricKey::generate(),
            0,
        );
        let id = contact.id().to_string();
        vauchi.add_contact(contact).unwrap();
        vauchi.add_contact_to_group(family.id(), &id).unwrap();
    }
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Groups);
    let groups = engine.initial_commands().unwrap();
    let (surface_id, family) = row(&groups, "Family");
    let detail = activate(&mut engine, surface_id, &family);
    (engine, detail)
}

fn member_rows(commands: &[Command]) -> Vec<String> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::List { id, rows, .. } if id.as_str() == "members" => {
                Some(rows.into_iter().map(|r| r.title))
            }
            _ => None,
        })
        .flatten()
        .collect()
}

fn group_info(commands: &[Command]) -> Vec<(String, Option<String>)> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::Group {
                label, children, ..
            } if label.as_deref() == Some("Group Info") => Some(children),
            _ => None,
        })
        .flatten()
        .filter_map(|node| match node {
            PresentationNode::Status { title, detail, .. } => Some((title, detail)),
            _ => None,
        })
        .collect()
}

fn surface_titles(commands: &[Command]) -> Vec<(String, String)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some((
                surface.surface_id.as_str().to_owned(),
                surface.title.clone(),
            )),
            _ => None,
        })
        .collect()
}

// @scenario: navigation :: Group detail shows member list
#[test]
fn the_group_screen_lists_its_members_and_the_entries_it_sees() {
    let (_, detail) = family_screen(&["Bob", "Carol"]);

    let mut members = member_rows(&detail);
    members.sort();

    assert_eq!(members, ["Bob", "Carol"]);
    assert_eq!(
        group_info(&detail),
        [
            ("Contacts".to_string(), Some("2".to_string())),
            ("Sees".to_string(), Some("1 entry".to_string())),
        ]
    );
}

// @scenario: navigation :: Preview as group member
#[test]
fn previewing_as_a_member_opens_my_card_as_they_see_it() {
    let (mut engine, detail) = family_screen(&["Bob"]);
    let (surface_id, actions) = action(&detail, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &actions);
    let (surface_id, preview) = action(&menu, "Preview as Bob").expect("Preview as Bob");

    let shown = activate(&mut engine, surface_id, &preview);

    assert_eq!(
        surface_titles(&shown),
        [("my_info".to_string(), "Viewing as Bob".to_string())]
    );
}
