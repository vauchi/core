// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sender-timestamp boundary tests for the shared card-update pipeline.

use crate::sync_card_update_tests::{create_valid_update_at, setup_exchange_with_ratchets};
use vauchi_core::api::process_single_card_update;
use vauchi_core::contact_card::ContactCard;

// @internal
#[test]
fn far_future_sender_timestamp_falls_back_to_receive_time() {
    let (alice_wb, bob_wb, _shared_secret, bob_contact_id, alice_contact_id) =
        setup_exchange_with_ratchets();
    let alice_signing_pk = *alice_wb.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let new_card = ContactCard::new("Bob Updated");
    let before = alice_wb.storage().clock().unix_seconds();
    let far_future = before + 86_400;
    let ciphertext = create_valid_update_at(
        &bob_wb,
        &alice_signing_pk,
        &alice_contact_id,
        &old_card,
        &new_card,
        far_future,
    );

    process_single_card_update(
        alice_wb.identity().unwrap(),
        alice_wb.storage(),
        &bob_contact_id,
        &ciphertext,
    )
    .expect("a valid signed update with clock skew still applies");

    let after = alice_wb.storage().clock().unix_seconds();
    let stored = alice_wb
        .storage()
        .contacts()
        .load_contact(&bob_contact_id)
        .unwrap()
        .unwrap();
    let applied_at = stored.card_updated_at().unwrap();
    assert!(
        (before..=after).contains(&applied_at),
        "far-future sender time must fall back to local receive time, got {applied_at}"
    );
}
