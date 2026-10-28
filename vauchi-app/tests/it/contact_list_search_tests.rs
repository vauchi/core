// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Searching the contacts list by a field value, not only by name.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn save(vauchi: &Vauchi, name: &str, fields: &[(FieldType, &str, &str)]) {
    let mut card = ContactCard::new(name);
    for (kind, label, value) in fields {
        card.add_field(ContactField::new(kind.clone(), label, value, 0))
            .unwrap();
    }
    let contact = Contact::from_exchange(
        *Identity::create(name, 0).signing_public_key(),
        card,
        SymmetricKey::generate(),
        0,
    );
    vauchi.storage().contacts().save_contact(&contact).unwrap();
}

/// (name, subtitle) of each row the contacts list shows.
fn rows(result: ActionResult) -> Vec<(String, Option<String>)> {
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("a search re-renders the list, got {result:?}");
    };
    screen
        .components
        .into_iter()
        .find_map(|c| match c {
            Component::List { id, items, .. } if id == "contacts" => Some(items),
            _ => None,
        })
        .unwrap()
        .into_iter()
        .map(|item| (item.name, item.subtitle))
        .collect()
}

// @scenario: contacts_management :: Search contacts by field value
#[test]
fn searching_a_phone_number_lists_its_contact_with_that_number_shown() {
    let vauchi = Vauchi::in_memory().unwrap();
    save(
        &vauchi,
        "Bob",
        &[
            (FieldType::Email, "Work", "bob@example.com"),
            (FieldType::Phone, "Mobile", "+41 79 555-1234"),
        ],
    );
    save(
        &vauchi,
        "Carol",
        &[(FieldType::Phone, "Mobile", "+41 79 555-9876")],
    );
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);

    let result = engine.handle_action(UserAction::SearchChanged {
        component_id: "contact_search".into(),
        query: "555-1234".into(),
    });

    assert_eq!(
        rows(result),
        vec![("Bob".to_string(), Some("+41 79 555-1234".to_string()))]
    );
}
