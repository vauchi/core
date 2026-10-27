// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! My Card's "Preview as..." row opens the contact list as a picker:
//! choosing a contact there shows my card as that contact sees it, not
//! their detail screen (private#858).

use crate::event_path_support::{action, activate, row, surface_titles};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::{Command, Contact, ContactCard, Identity, SymmetricKey};

/// Alice, with one "Work" email, has exchanged cards with Bob. Returns
/// the engine on My Card, My Card's commands, and Bob's contact id.
fn my_card_with_bob() -> (AppEngine, Vec<Command>, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Work",
            "alice@example.org",
            0,
        ))
        .unwrap();
    let bob = Identity::create("Bob", 0);
    let contact = Contact::from_exchange(
        *bob.signing_public_key(),
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    (engine, my_card, bob_id)
}

// @scenario: navigation :: Preview my card as a specific contact
#[test]
fn picking_a_contact_under_preview_as_shows_my_card_as_they_see_it() {
    let (mut engine, my_card, _) = my_card_with_bob();
    let (surface_id, preview_as) = row(&my_card, "Preview as...");

    let picker = activate(&mut engine, surface_id, &preview_as);
    assert_eq!(
        surface_titles(&picker),
        [
            ("my_info".to_string(), "Alice".to_string()),
            ("contact_picker".to_string(), "Preview as...".to_string()),
        ],
        "the picker is a detail of My Card, which stays beside it on wide layouts"
    );
    for label in ["Add Contact", "Actions"] {
        assert!(
            action(&picker, label).is_none(),
            "a picker only picks; it offers no {label}: {picker:#?}"
        );
    }

    let (surface_id, bob) = row(&picker, "Bob");
    let shown = activate(&mut engine, surface_id, &bob);
    assert_eq!(
        surface_titles(&shown),
        [("my_info".to_string(), "Viewing as Bob".to_string())]
    );
    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
}

// @scenario: navigation :: Preview my card as a specific contact
#[test]
fn leaving_the_picker_makes_the_next_contact_tap_an_ordinary_open() {
    let (mut engine, my_card, bob_id) = my_card_with_bob();
    let (surface_id, preview_as) = row(&my_card, "Preview as...");
    activate(&mut engine, surface_id, &preview_as);
    engine.navigate_back();
    engine.navigate_to(AppScreen::Contacts);
    let contacts = engine.initial_commands().unwrap();
    assert_eq!(
        surface_titles(&contacts),
        [("contacts".to_string(), "Contacts".to_string())],
        "the ordinary list is not a picker"
    );

    let (surface_id, bob) = row(&contacts, "Bob");
    activate(&mut engine, surface_id, &bob);

    assert_eq!(
        engine.current_app_screen(),
        &AppScreen::ContactDetail { contact_id: bob_id }
    );
}
