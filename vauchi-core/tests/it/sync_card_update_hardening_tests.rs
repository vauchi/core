// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Defensive card-update tests that exercise ratchet failure boundaries.

use crate::sync_card_update_tests::{create_valid_update, setup_exchange_with_ratchets};
use vauchi_core::api::{CardUpdateError, ReceiveOutcome, process_single_card_update};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::ratchet::RatchetMessage;

// @scenario: security :: Failed authentication does not persist ratchet mutation
#[test]
fn test_crypto_hardening_card_update_auth_failure_preserves_stored_ratchet() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@rollback.test",
            0,
        ))
        .unwrap();
    let genuine = create_valid_update(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
    );
    let mut tampered: RatchetMessage = serde_json::from_slice(&genuine).unwrap();
    let last = tampered.ciphertext.len() - 1;
    tampered.ciphertext[last] ^= 0x80;
    let tampered = serde_json::to_vec(&tampered).unwrap();

    let (before, _) = alice_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_contact_id)
        .unwrap()
        .unwrap();
    let before = serde_json::to_vec(&before.serialize()).unwrap();

    let rejected = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &tampered,
    );
    assert!(matches!(rejected, Err(CardUpdateError::DecryptionFailed)));

    let (after, _) = alice_wb
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_contact_id)
        .unwrap()
        .unwrap();
    let after = serde_json::to_vec(&after.serialize()).unwrap();
    assert_eq!(
        after, before,
        "failed decrypt must not persist ratchet mutation"
    );

    let retried = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &genuine,
    );
    assert!(matches!(retried, Ok(ReceiveOutcome::CardDelta)));
    let contact = alice_wb.get_contact(&bob_contact_id).unwrap().unwrap();
    assert!(
        contact
            .card()
            .fields()
            .iter()
            .any(|field| field.value() == "bob@rollback.test")
    );
}
