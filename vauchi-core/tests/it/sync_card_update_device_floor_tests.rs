// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the per-(contact, peer-device) stale-version floor
//! (sync/card_update.rs), split out of `sync_card_update_tests.rs` to keep
//! that file under the 1200-line size gate.

use crate::sync_card_update_tests::{
    create_valid_update_versioned, create_valid_update_versioned_for_device,
    setup_exchange_with_ratchets,
};
use vauchi_core::api::{
    CardUpdateError, process_single_card_update, process_single_card_update_for_device,
};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;

// A multi-device sender numbers deltas per device, from its own storage.
// Keying the receiver's stale-version floor per CONTACT therefore breaks
// convergence: device A1's applied v2 raises the floor to 2, and device A2's
// first-ever send (v1 on its fresh channel) is rejected as stale even though
// it carries the legitimate newest card. The floor must be per
// (contact, device): A2's v1 applies, while A1's own withheld v1 stays stale.
// @scenario: sync_updates :: A multi-device sender's versions are floored per device
#[test]
fn delta_version_floor_is_tracked_per_peer_device() {
    let (alice_wb, bob_wb, shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();

    let base = ContactCard::new("Bob");

    // Sender device A1 (the legacy session) applies its v2 edit.
    let mut card_v2 = ContactCard::new("Bob");
    card_v2
        .add_field(ContactField::new(
            FieldType::Phone,
            "Phone",
            "+41 79 000",
            0,
        ))
        .unwrap();
    let ct_a1_v2 = create_valid_update_versioned(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &base,
        &card_v2,
        2,
    );
    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ct_a1_v2,
    )
    .expect("A1's v2 update must apply");

    // Sender device A2 gets its own ratchet session (its versions count from
    // its own storage, independently of A1).
    let a2_device_id = [2; 32];
    let alice_dh_a2 = X3DHKeyPair::generate();
    let bob_ratchet_a2 =
        DoubleRatchetState::initialize_initiator(&shared_secret, *alice_dh_a2.public_key())
            .unwrap();
    let alice_ratchet_a2 = DoubleRatchetState::initialize_responder(&shared_secret, alice_dh_a2);
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(&alice_contact_id, &a2_device_id, &bob_ratchet_a2, true)
        .unwrap();
    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state_for_device(&bob_contact_id, &a2_device_id, &alice_ratchet_a2, false)
        .unwrap();

    // A2's first-ever send to Alice is stamped v1 — the legitimate newest
    // card on a fresh channel, not a downgrade.
    let mut card_a2_v1 = ContactCard::new("Bob");
    card_a2_v1
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "a2-first-send@x.com",
            0,
        ))
        .unwrap();
    let ct_a2_v1 = create_valid_update_versioned_for_device(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &a2_device_id,
        &base,
        &card_a2_v1,
        1,
    );
    process_single_card_update_for_device(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &a2_device_id,
        &ct_a2_v1,
    )
    .expect("A2's first-ever v1 must be accepted — A1's floor must not floor another device");

    // A1's own withheld v1 (reordered behind its v2) stays stale.
    let mut card_a1_v1 = ContactCard::new("Bob");
    card_a1_v1
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "a1-stale@x.com",
            0,
        ))
        .unwrap();
    let ct_a1_v1 = create_valid_update_versioned(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &base,
        &card_a1_v1,
        1,
    );
    let stale = process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ct_a1_v1,
    );
    assert!(
        matches!(
            stale,
            Err(CardUpdateError::StaleVersion { delta: 1, last: 2 })
        ),
        "A1's older v1 must still be rejected as StaleVersion{{delta:1,last:2}}; got {stale:?}"
    );

    // The stored card holds A1's v2 field and A2's field — and exactly one
    // Email (A1's stale Email never applied).
    let stored = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    assert!(
        stored.card().fields().iter().any(|f| f.label() == "Phone"),
        "A1's v2 Phone field must remain"
    );
    let emails: Vec<_> = stored
        .card()
        .fields()
        .iter()
        .filter(|f| f.label() == "Email")
        .collect();
    assert_eq!(
        emails.len(),
        1,
        "exactly one Email field must be stored (A1's stale one rejected)"
    );
    assert_eq!(
        emails[0].value(),
        "a2-first-send@x.com",
        "the stored Email must be A2's accepted v1 field"
    );
}
