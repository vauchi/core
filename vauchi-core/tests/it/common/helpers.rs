// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Test Helpers
//!
//! Common helper functions for setting up test scenarios.

use vauchi_app::theme::{Theme, load_themes_from_json};
use vauchi_core::{
    Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi,
    crypto::ratchet::DoubleRatchetState, exchange::X3DHKeyPair,
};

/// Load all themes from generated/themes.json at runtime.
/// Falls back to default theme if file not found (themes repo not checked out).
pub fn all_themes() -> Vec<Theme> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/generated/themes.json");
    match std::fs::read(&path) {
        Ok(data) => load_themes_from_json(&data).expect("generated/themes.json must be valid"),
        Err(_) => vec![vauchi_app::default_theme()],
    }
}

/// Load themes from generated/themes.json.
/// Returns None if file doesn't exist (themes repo not checked out / worktree).
pub fn try_all_themes() -> Option<Vec<Theme>> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../themes/generated/themes.json");
    let data = std::fs::read(&path).ok()?;
    Some(load_themes_from_json(&data).expect("generated/themes.json must be valid"))
}

/// Find a theme by ID from themes.json (replaces deprecated get_theme_by_id).
pub fn theme_by_id(id: &str) -> Option<Theme> {
    all_themes().into_iter().find(|t| t.id == id)
}

/// Create an in-memory Vauchi with an identity.
pub fn create_vauchi_with_identity(name: &str) -> Vauchi {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

/// Create a Vauchi with identity and some fields on the card.
pub fn create_vauchi_with_card(name: &str, fields: Vec<(FieldType, &str, &str)>) -> Vauchi {
    let wb = create_vauchi_with_identity(name);
    for (field_type, label, value) in fields {
        wb.add_own_field(ContactField::new(field_type, label, value, 0))
            .unwrap();
    }
    wb
}

/// Set up two Vauchis with a mutual contact relationship.
/// Returns (alice_wb, bob_wb, shared_secret, bob_contact_id_at_alice, alice_contact_id_at_bob)
pub fn setup_alice_bob_exchange() -> (Vauchi, Vauchi, SymmetricKey, String, String) {
    let alice_wb = create_vauchi_with_identity("Alice");
    let bob_wb = create_vauchi_with_identity("Bob");

    let alice_pk = *alice_wb.identity().unwrap().signing_public_key();
    let bob_pk = *bob_wb.identity().unwrap().signing_public_key();

    let shared_secret = SymmetricKey::generate();

    // Alice adds Bob as contact
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice_wb.add_contact(bob_contact).unwrap();

    // Bob adds Alice as contact
    let alice_card = alice_wb
        .own_card()
        .unwrap()
        .unwrap_or_else(|| ContactCard::new("Alice"));
    let alice_contact = Contact::from_exchange(alice_pk, alice_card, shared_secret.clone(), 0);
    let alice_contact_id = alice_contact.id().to_string();
    bob_wb.add_contact(alice_contact).unwrap();

    (
        alice_wb,
        bob_wb,
        shared_secret,
        bob_contact_id,
        alice_contact_id,
    )
}

/// Set up ratchet states for Alice and Bob.
/// Returns (alice_ratchet, bob_ratchet)
pub fn setup_ratchets(shared_secret: &SymmetricKey) -> (DoubleRatchetState, DoubleRatchetState) {
    let bob_dh = X3DHKeyPair::generate();
    let alice_ratchet =
        DoubleRatchetState::initialize_initiator(shared_secret, *bob_dh.public_key()).unwrap();
    let bob_ratchet = DoubleRatchetState::initialize_responder(shared_secret, bob_dh);
    (alice_ratchet, bob_ratchet)
}

/// Set up three users with mutual contacts.
/// Returns (alice, bob, carol, secrets_map)
#[allow(clippy::type_complexity)]
pub fn setup_three_users() -> (
    Vauchi,
    Vauchi,
    Vauchi,
    std::collections::HashMap<(String, String), SymmetricKey>,
) {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let carol = create_vauchi_with_identity("Carol");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let carol_pk = *carol.identity().unwrap().signing_public_key();

    let mut secrets = std::collections::HashMap::new();

    // Alice <-> Bob
    let ab_secret = SymmetricKey::generate();
    secrets.insert(("alice".to_string(), "bob".to_string()), ab_secret.clone());
    alice
        .add_contact(Contact::from_exchange(
            bob_pk,
            ContactCard::new("Bob"),
            ab_secret.clone(),
            0,
        ))
        .unwrap();
    bob.add_contact(Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        ab_secret,
        0,
    ))
    .unwrap();

    // Alice <-> Carol
    let ac_secret = SymmetricKey::generate();
    secrets.insert(
        ("alice".to_string(), "carol".to_string()),
        ac_secret.clone(),
    );
    alice
        .add_contact(Contact::from_exchange(
            carol_pk,
            ContactCard::new("Carol"),
            ac_secret.clone(),
            0,
        ))
        .unwrap();
    carol
        .add_contact(Contact::from_exchange(
            alice_pk,
            ContactCard::new("Alice"),
            ac_secret,
            0,
        ))
        .unwrap();

    // Bob <-> Carol
    let bc_secret = SymmetricKey::generate();
    secrets.insert(("bob".to_string(), "carol".to_string()), bc_secret.clone());
    bob.add_contact(Contact::from_exchange(
        carol_pk,
        ContactCard::new("Carol"),
        bc_secret.clone(),
        0,
    ))
    .unwrap();
    carol
        .add_contact(Contact::from_exchange(
            bob_pk,
            ContactCard::new("Bob"),
            bc_secret,
            0,
        ))
        .unwrap();

    (alice, bob, carol, secrets)
}

/// Assert that a Vauchi has a specific number of contacts.
pub fn assert_contact_count(wb: &Vauchi, expected: usize) {
    let contacts = wb.storage().list_contacts().unwrap();
    assert_eq!(
        contacts.len(),
        expected,
        "Expected {} contacts, found {}",
        expected,
        contacts.len()
    );
}

/// Assert that a card has a field with specific value.
pub fn assert_card_has_field(card: &ContactCard, label: &str, expected_value: &str) {
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label)
        .unwrap_or_else(|| panic!("Field '{}' not found in card", label));
    assert_eq!(
        field.value(),
        expected_value,
        "Field '{}' has value '{}', expected '{}'",
        label,
        field.value(),
        expected_value
    );
}
