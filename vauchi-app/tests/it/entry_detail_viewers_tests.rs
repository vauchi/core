// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The entry-detail screen answers "who can see this entry" — it must
//! name exactly the contacts the card is sent to (#425). An entry shared
//! with all contacts was reported as seen by no one, because the list was
//! built from group grants alone.

use vauchi_app::ui::{AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{Identity, Vauchi};

const NOW: u64 = 1_790_000_000;
const NOBODY: &str = "No contacts can see this entry.";

fn exchanged(vauchi: &Vauchi, name: &str) -> String {
    let peer = Identity::create(name, NOW);
    let contact = Contact::from_exchange(
        *peer.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        NOW,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).expect("add contact");
    id
}

/// Ada with one phone entry and two exchanged contacts, no groups.
fn world() -> (Vauchi, String, Vec<String>) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Core");
    vauchi.create_identity("Ada").expect("identity");
    let field = ContactField::new(FieldType::Phone, "mobile", "+41 79 000 00 00", NOW);
    let field_id = field.id().to_string();
    vauchi.add_own_field(field).expect("own field");
    let contacts = vec![exchanged(&vauchi, "Grace"), exchanged(&vauchi, "Alan")];
    (vauchi, field_id, contacts)
}

fn texts(screen: &ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

/// (name, reason) for each row of the "visible to" list.
fn viewers(screen: &ScreenModel) -> Vec<(String, Option<String>)> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::ActionList { id, items } if id == "visible_contacts" => Some(items),
            _ => None,
        })
        .flatten()
        .map(|item| (item.label.clone(), item.detail.clone()))
        .collect()
}

fn open_entry(vauchi: Vauchi, field_id: &str) -> (AppEngine, ScreenModel) {
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::MyInfoEntryDetail {
        field_id: field_id.to_string(),
    });
    (engine, screen)
}

// @internal
#[test]
fn an_entry_shared_with_all_contacts_lists_every_contact() {
    let (vauchi, field_id, _) = world();
    vauchi
        .set_field_shown(&field_id, true)
        .expect("share with all");

    let (_, screen) = open_entry(vauchi, &field_id);

    assert!(
        !texts(&screen).iter().any(|t| t == NOBODY),
        "{:?}",
        texts(&screen)
    );
    let mut rows = viewers(&screen);
    rows.sort();
    assert_eq!(
        rows,
        vec![
            ("Alan".to_string(), Some("All contacts".to_string())),
            ("Grace".to_string(), Some("All contacts".to_string())),
        ]
    );
}

// @internal
#[test]
fn a_hidden_entry_still_says_no_one_can_see_it() {
    let (vauchi, field_id, _) = world();
    vauchi.set_field_shown(&field_id, false).expect("hide");

    let (_, screen) = open_entry(vauchi, &field_id);

    assert!(
        texts(&screen).iter().any(|t| t == NOBODY),
        "{:?}",
        texts(&screen)
    );
    assert!(viewers(&screen).is_empty());
}

// @internal
#[test]
fn a_group_entry_lists_only_the_group_members() {
    let (vauchi, field_id, contacts) = world();
    let family = vauchi.create_group("Family").expect("group");
    vauchi
        .add_contact_to_group(family.id(), &contacts[0])
        .expect("member");
    vauchi
        .set_group_field_visibility(family.id(), &field_id, true)
        .expect("grant");

    let (_, screen) = open_entry(vauchi, &field_id);

    assert_eq!(
        viewers(&screen),
        vec![("Grace".to_string(), Some("via Family".to_string()))]
    );
}

// @internal
#[test]
fn sharing_with_all_contacts_refreshes_the_list_at_once() {
    let (vauchi, field_id, _) = world();
    vauchi.set_field_shown(&field_id, false).expect("hide");
    let (mut engine, _) = open_entry(vauchi, &field_id);

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "entry_visibility".into(),
        item_id: "shown".into(),
    });
    let screen = engine.current_screen();

    assert!(
        !texts(&screen).iter().any(|t| t == NOBODY),
        "{:?}",
        texts(&screen)
    );
    assert_eq!(viewers(&screen).len(), 2);
}
