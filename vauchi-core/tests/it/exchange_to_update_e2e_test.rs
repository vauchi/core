// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange-to-Update Full-Path E2E Test
//!
//! Verifies the complete production pipeline in a single test:
//! exchange → ratchet init → storage → card update → prepare (CEK+sign+encrypt)
//! → process (decrypt+verify+apply) → storage persistence.
//!
//! Existing tests cover these steps individually with manual wiring between them.
//! This test exercises the API-level functions (`prepare_card_update_for_contact`,
//! `process_card_update`) to verify the handoff seams.
//!
//! Feature: contact_exchange.feature, sync_updates.feature

use crate::common;

use common::helpers::{create_vauchi_with_card, setup_ratchets};
use vauchi_core::{Contact, ContactField, FieldType};

/// Full pipeline: exchange → ratchet → prepare_card_update → process_card_update → verify.
///
/// This is the single test that guards every seam in the update propagation path.
/// A regression in CEK wrapping, signature binding, ratchet state persistence,
/// delta computation, or field-level application would fail here.
// @scenario: contact_exchange :: Successful QR code exchange with proximity
// @scenario: sync_updates :: Update propagates to contacts
#[test]
fn test_full_path_exchange_to_card_update() {
    // Step 1: Create Alice and Bob with identity + card fields
    let alice = create_vauchi_with_card(
        "Alice",
        vec![
            (FieldType::Email, "work", "alice@old.com"),
            (FieldType::Phone, "mobile", "+15551111111"),
        ],
    );
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@email.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // Step 2: Exchange contacts (simulating completed ExchangeSession output)
    let shared_secret = vauchi_core::SymmetricKey::generate();

    let bob_contact = Contact::from_exchange(bob_pk, bob_card, shared_secret.clone(), 0);
    let bob_contact_id = bob_contact.id().to_string();
    alice.add_contact(bob_contact).unwrap();

    let alice_contact =
        Contact::from_exchange(alice_pk, alice_card.clone(), shared_secret.clone(), 0);
    let alice_contact_id = alice_contact.id().to_string();
    bob.add_contact(alice_contact).unwrap();

    // Step 3: Initialize and persist ratchets (production does this at exchange completion)
    let (alice_ratchet, bob_ratchet) = setup_ratchets(&shared_secret);
    alice
        .storage()
        .save_ratchet_state(&bob_contact_id, &alice_ratchet, true)
        .unwrap();
    bob.storage()
        .save_ratchet_state(&alice_contact_id, &bob_ratchet, false)
        .unwrap();

    // Step 4: Alice updates her card (changes email, adds a new field)
    let old_card = alice.own_card().unwrap().unwrap();
    let email_field_id = old_card
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .unwrap()
        .id()
        .to_string();
    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&email_field_id, "alice@new-company.com", 0)
        .unwrap();
    alice.update_own_card(&new_card).unwrap();

    alice
        .add_own_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://alice.example",
            0,
        ))
        .unwrap();
    let new_card = alice.own_card().unwrap().unwrap();

    // Step 5: Alice prepares encrypted update via production API
    let encrypted = alice
        .prepare_card_update_for_contact(&bob_contact_id, &old_card, &new_card)
        .expect("prepare_card_update_for_contact should succeed");

    assert!(!encrypted.is_empty(), "encrypted payload must be non-empty");

    // Step 6: Bob processes the update via production API
    let changed_fields = bob
        .process_card_update(&alice_contact_id, &encrypted)
        .expect("process_card_update should succeed");

    assert!(
        !changed_fields.is_empty(),
        "at least one field should have changed"
    );

    // Step 7: Verify Bob's copy of Alice's card reflects the update
    let alice_at_bob = bob.get_contact(&alice_contact_id).unwrap().unwrap();
    let updated_card = alice_at_bob.card();

    let email = updated_card
        .fields()
        .iter()
        .find(|f| f.label() == "work")
        .expect("work email field must exist");
    assert_eq!(
        email.value(),
        "alice@new-company.com",
        "email must reflect the update"
    );

    let blog = updated_card
        .fields()
        .iter()
        .find(|f| f.label() == "blog")
        .expect("blog field must exist after update");
    assert_eq!(blog.value(), "https://alice.example");

    // Phone field should be unchanged
    let phone = updated_card
        .fields()
        .iter()
        .find(|f| f.label() == "mobile")
        .expect("mobile phone field must survive the update");
    assert_eq!(phone.value(), "+15551111111");
}
