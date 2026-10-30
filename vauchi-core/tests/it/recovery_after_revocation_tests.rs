// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A thief's revocation must not block the real recovery
//! (vauchi/private#935, SC-16): the contact's CEK and card go, but the
//! tombstone keeps the key and the name, so the contact can still vouch
//! for a claim on that key.

use base64::Engine as _;
use vauchi_core::Vauchi;
use vauchi_core::VauchiError;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::identity::Identity;
use vauchi_core::network::message::IdentityRevoked;
use vauchi_core::network::revocation::process_revocation;
use vauchi_core::recovery::{RecoveryClaim, RecoveryVoucher};

const EXCHANGED_AT: u64 = 1_700_000_000;

struct World {
    bob: Vauchi,
    alice_old: Identity,
    alice_id: String,
}

/// Bob exchanged with Alice; Alice's old device, now in a thief's hands,
/// revokes her identity at Bob.
fn bob_after_alices_revocation() -> World {
    let mut bob = Vauchi::in_memory().unwrap();
    bob.create_identity("Bob").unwrap();
    let alice_old = Identity::create("Alice", 0);
    let contact = Contact::from_exchange(
        *alice_old.signing_public_key(),
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        EXCHANGED_AT,
    );
    let alice_id = contact.id().to_string();
    bob.storage().contacts().save_contact(&contact).unwrap();
    let bob_id = hex::encode(bob.identity().unwrap().signing_public_key());
    let revocation = IdentityRevoked::create(&alice_old, &bob_id, EXCHANGED_AT + 1);

    assert!(
        process_revocation(&revocation, bob.storage()).unwrap(),
        "the thief's revocation verifies: it is signed by Alice's key"
    );
    World {
        bob,
        alice_old,
        alice_id,
    }
}

fn claim_for(old: &Identity, now: u64) -> (RecoveryClaim, String) {
    let new_alice = Identity::create("Alice New", 0);
    let claim = RecoveryClaim::new(
        old.signing_public_key(),
        new_alice.signing_public_key(),
        now,
    );
    let b64 = base64::engine::general_purpose::STANDARD.encode(claim.to_bytes());
    (claim, b64)
}

// @scenario: contact_recovery :: A revoked contact can still vouch for the real recovery
#[test]
fn a_revoked_contact_can_still_vouch_for_the_real_recovery() {
    let w = bob_after_alices_revocation();
    assert!(
        w.bob
            .storage()
            .contacts()
            .load_contact(&w.alice_id)
            .unwrap()
            .is_none(),
        "the card and its key are gone"
    );
    assert!(
        w.bob
            .storage()
            .contacts()
            .is_sender_revoked(&w.alice_id)
            .unwrap()
    );
    let (claim, claim_b64) = claim_for(&w.alice_old, w.bob.clock().unix_seconds());

    let voucher_b64 = w
        .bob
        .create_voucher_from_claim_b64(&claim_b64)
        .expect("Bob vouches for the key he exchanged with, revoked or not");

    let voucher = RecoveryVoucher::from_bytes(
        &base64::engine::general_purpose::STANDARD
            .decode(voucher_b64)
            .unwrap(),
    )
    .unwrap();
    assert!(voucher.verify());
    assert_eq!(voucher.old_pk(), w.alice_old.signing_public_key());
    assert_eq!(
        voucher.voucher_pk(),
        w.bob.identity().unwrap().signing_public_key()
    );
    assert_eq!(
        w.bob.recovery_claim_contact(&claim).unwrap().display_name(),
        "Alice",
        "the tombstone still names her"
    );
}

// @scenario: contact_recovery :: Voucher does not recognize the claimed identity
#[test]
fn a_claim_for_a_key_bob_never_met_is_still_refused_after_a_revocation() {
    let w = bob_after_alices_revocation();
    let stranger = Identity::create("Stranger", 0);
    let (_, claim_b64) = claim_for(&stranger, w.bob.clock().unix_seconds());

    let refused = w.bob.create_voucher_from_claim_b64(&claim_b64);

    assert!(
        matches!(refused, Err(VauchiError::ContactNotFound(_))),
        "{refused:?}"
    );
}

// @scenario: contact_recovery :: A revoked contact can still vouch for the real recovery
#[test]
fn the_tombstone_keeps_the_name_encrypted_at_rest() {
    let w = bob_after_alices_revocation();

    let stored: Vec<u8> = w
        .bob
        .storage()
        .connection()
        .query_row(
            "SELECT display_name_encrypted FROM revoked_senders WHERE sender_id = ?1",
            [&w.alice_id],
            |row| row.get(0),
        )
        .expect("the tombstone row carries a name");

    assert!(!stored.is_empty());
    assert!(
        !stored.windows(5).any(|window| window == b"Alice"),
        "the name is not stored in the clear (ADR-015)"
    );
}
