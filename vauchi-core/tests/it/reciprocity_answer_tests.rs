// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A confirmed contact answers a peer still waiting for confirmation
//! (private#663). A confirmation says whether its sender is still waiting;
//! only a waiting sender is answered, so two confirmed phones never answer
//! each other.

use crate::common;

use common::helpers::create_vauchi_with_identity;
use common::linked_peer::{LinkedPeer, link_contacts};
use vauchi_core::Vauchi;
use vauchi_core::api::vauchi::{BlobOutcome, process_received_blobs};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::reciprocity::Reciprocity;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::sync::delta::{ReciprocityConfirmPayload, VersionedPayload};

/// Alice already counts Bob as confirmed; Bob still waits for Alice. Bob is
/// the ratchet initiator, so his confirmation goes first.
fn alice_confirmed_bob_waiting() -> (Vauchi, Vauchi, LinkedPeer) {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    let now = alice.storage().clock().unix_seconds();
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    // Re-saved with a recent exchange time: at time 0 the 7-day timer reads
    // the exchange as Unreciprocated, which never sends.
    let mut bob_of_alice = Contact::from_exchange(
        alice_pk,
        ContactCard::new("Alice"),
        link.shared_secret.clone(),
        now,
    );
    bob_of_alice.set_reciprocity(Reciprocity::Pending);
    bob.storage()
        .contacts()
        .save_contact(&bob_of_alice)
        .unwrap();

    let mut alice_of_bob = alice.get_contact(&link.peer_contact_id).unwrap().unwrap();
    alice_of_bob.set_reciprocity(Reciprocity::Confirmed);
    alice
        .storage()
        .contacts()
        .save_contact(&alice_of_bob)
        .unwrap();

    (alice, bob, link)
}

/// Delivers every update `sender` has queued to `recipient`, as the relay
/// would under the recipient's mailbox token.
fn deliver_queued(sender: &Vauchi, recipient: &Vauchi, link: &LinkedPeer) -> Vec<BlobOutcome> {
    let recipient_pk = *recipient.identity().unwrap().signing_public_key();
    let token = token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        &recipient_pk,
        current_day_epoch(recipient.storage().clock().unix_seconds()),
    ));
    let pending = sender
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let blobs = pending
        .into_iter()
        .map(|update| {
            sender
                .storage()
                .pending()
                .mark_update_sent(&update.id)
                .unwrap();
            (update.id, token.clone(), update.payload, None)
        })
        .collect();
    let contacts = recipient.storage().contacts().list_contacts().unwrap();
    process_received_blobs(
        recipient.identity().unwrap(),
        recipient.storage(),
        &contacts,
        blobs,
    )
}

fn reciprocity(owner: &Vauchi, contact_id: &str) -> Reciprocity {
    let now = owner.storage().clock().unix_seconds();
    owner
        .get_contact(contact_id)
        .unwrap()
        .unwrap()
        .reciprocity(now)
}

// The rig case (2026-10-12): Bob's one confirmation to Alice was lost, so
// Bob waits and re-sends on every sync while Alice is already confirmed.
// @scenario: receive_phase :: a confirmed contact answers a peer still waiting
// @internal
#[test]
fn a_confirmed_contact_answers_a_waiting_peer_and_the_peer_confirms() {
    let (alice, bob, link) = alice_confirmed_bob_waiting();
    assert_eq!(bob.queue_reciprocity_confirmations().unwrap(), 1);

    let at_alice = deliver_queued(&bob, &alice, &link);
    assert_eq!(
        at_alice[0].reciprocity_answer.as_deref(),
        Some(link.peer_contact_id.as_str()),
        "a confirmation from a waiting peer asks for an answer"
    );
    assert_eq!(
        alice
            .queue_reciprocity_answer(&link.peer_contact_id)
            .unwrap(),
        1
    );

    let at_bob = deliver_queued(&alice, &bob, &link);

    assert!(at_bob[0].decrypted, "the answer decrypts and verifies");
    assert_eq!(
        reciprocity(&bob, &link.host_contact_id),
        Reciprocity::Confirmed,
        "the answer confirms the waiting peer"
    );
    assert_eq!(
        at_bob[0].reciprocity_answer, None,
        "an answer is never answered"
    );
    assert_eq!(
        bob.queue_reciprocity_confirmations().unwrap(),
        0,
        "the confirmed peer stops re-sending"
    );
}

// CC-14: a waiting confirmation over the wrong token is from no one we
// exchanged with, so it is not answered.
// @scenario: receive_phase :: a wrong-token confirmation is not answered
// @internal
#[test]
fn a_waiting_confirmation_with_the_wrong_token_is_not_answered() {
    let (alice, bob, link) = alice_confirmed_bob_waiting();
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let forged = ReciprocityConfirmPayload::new([0xFF; 32], bob.identity().unwrap(), &alice_pk);
    let (mut ratchet, is_initiator) = bob
        .storage()
        .ratchets()
        .load_ratchet_state(&link.host_contact_id)
        .unwrap()
        .unwrap();
    let message = ratchet
        .encrypt(&VersionedPayload::encode_reciprocity(&forged))
        .unwrap();
    bob.storage()
        .ratchets()
        .save_ratchet_state(&link.host_contact_id, &ratchet, is_initiator)
        .unwrap();
    let token = token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        &alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
    ));
    let contacts = alice.storage().contacts().list_contacts().unwrap();

    let outcomes = process_received_blobs(
        alice.identity().unwrap(),
        alice.storage(),
        &contacts,
        vec![(
            "forged".to_string(),
            token,
            serde_json::to_vec(&message).unwrap(),
            None,
        )],
    );

    assert!(
        outcomes[0].decrypted,
        "the forged token is signed and decrypts"
    );
    assert_eq!(outcomes[0].reciprocity_answer, None);
}

// CC-14: an answer's signature must not pass as a waiting confirmation's, or a
// replayed answer would demand another answer.
// @internal
#[test]
fn an_answer_relabelled_as_a_waiting_confirmation_fails_verification() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let answer = ReciprocityConfirmPayload::answer([7; 32], alice.identity().unwrap(), &bob_pk);
    let mut wire = VersionedPayload::encode_reciprocity(&answer);
    assert_ne!(
        wire[0],
        vauchi_core::sync::delta::PAYLOAD_VERSION_RECIPROCITY
    );

    wire[0] = vauchi_core::sync::delta::PAYLOAD_VERSION_RECIPROCITY;

    let Ok(VersionedPayload::ReciprocityConfirm(relabelled)) = VersionedPayload::decode(&wire)
    else {
        panic!("the relabelled bytes still decode as a confirmation");
    };
    assert!(relabelled.sender_waiting());
    assert!(!relabelled.verify(&alice_pk, &bob_pk));
    assert!(answer.verify(&alice_pk, &bob_pk));
}
