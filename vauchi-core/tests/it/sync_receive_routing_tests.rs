// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Receive-phase routing integration tests.
//!
//! Exercises `process_received_blobs` end-to-end:
//! - mailbox-token attribution → O(1) fast path
//! - missing/unknown attribution → dropped (post-Step 2)
//! - replay rejection: same blob twice yields one decrypt, then drop
//! - mixed batches: per-blob outcomes returned in input order
//! - card delta is applied to the resolved contact
//!
//! Traces to: `_private/docs/problems/done/2026-04-27-sync-receive-quadratic-contacts/`
//! Decision: ADR-029 addendum 2026-04-27

use crate::common;

use common::helpers::create_vauchi_with_identity;
use vauchi_core::SymmetricKey;
use vauchi_core::api::vauchi::process_received_blobs;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::sync::delta::{CardDelta, CekWrappedPayload, VersionedPayload};

/// Add an exchanged contact `peer` to `host`. Returns the contact id and
/// the shared secret. Sets up Double Ratchet state on both sides so
/// `host` can decrypt updates from `peer`.
fn link_contacts(
    host: &vauchi_core::Vauchi,
    peer: &vauchi_core::Vauchi,
    label: &str,
) -> LinkedPeer {
    let host_pk = *host.identity().unwrap().signing_public_key();
    let peer_pk = *peer.identity().unwrap().signing_public_key();
    let shared_secret = SymmetricKey::generate();

    let peer_at_host =
        Contact::from_exchange(peer_pk, ContactCard::new(label), shared_secret.clone());
    let peer_contact_id = peer_at_host.id().to_string();
    host.add_contact(peer_at_host).unwrap();

    let host_at_peer =
        Contact::from_exchange(host_pk, ContactCard::new("host"), shared_secret.clone());
    let host_contact_id = host_at_peer.id().to_string();
    peer.add_contact(host_at_peer).unwrap();

    let host_dh = X3DHKeyPair::generate();
    let peer_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *host_dh.public_key()).unwrap();
    let host_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, host_dh);

    host.storage()
        .save_ratchet_state(&peer_contact_id, &host_ratchet, false)
        .unwrap();
    peer.storage()
        .save_ratchet_state(&host_contact_id, &peer_ratchet, true)
        .unwrap();

    LinkedPeer {
        peer_contact_id,
        host_contact_id,
        shared_secret,
    }
}

struct LinkedPeer {
    peer_contact_id: String,
    host_contact_id: String,
    shared_secret: SymmetricKey,
}

/// Encrypt a card update from `sender` to `recipient`, returning the
/// JSON-serialized RatchetMessage that flows over the wire.
fn encrypt_update(
    sender: &vauchi_core::Vauchi,
    recipient_signing_pk: &[u8; 32],
    recipient_contact_id: &str,
    old_card: &ContactCard,
    new_card: &ContactCard,
) -> Vec<u8> {
    let sender_identity = sender.identity().unwrap();
    let mut delta = CardDelta::compute(old_card, new_card);
    delta.sign(sender_identity, recipient_signing_pk);

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let cek = ContentEncryptionKey::generate();
    let cek_ciphertext = cek.encrypt(&delta_bytes).unwrap();
    let wrapped = CekWrappedPayload {
        cek: cek.to_bytes(),
        cek_ciphertext,
        signature: delta.signature,
        nonce: delta.nonce,
    };
    let payload = VersionedPayload::encode_cek(&wrapped);

    let (mut sender_ratchet, is_init) = sender
        .storage()
        .load_ratchet_state(recipient_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = sender_ratchet.encrypt(&payload).unwrap();
    sender
        .storage()
        .save_ratchet_state(recipient_contact_id, &sender_ratchet, is_init)
        .unwrap();
    serde_json::to_vec(&ratchet_msg).unwrap()
}

// @scenario: receive_phase :: mailbox_token attribution drives O(1) routing
/// In-spec input: relay attributes the blob to its mailbox token, the
/// receive loop resolves to Bob in O(1) and applies the update.
// @internal
#[test]
fn test_receive_routes_via_mailbox_token_fast_path() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let charlie = create_vauchi_with_identity("Charlie");

    let bob_link = link_contacts(&alice, &bob, "Bob");
    let _charlie_link = link_contacts(&alice, &charlie, "Charlie");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(FieldType::Email, "Email", "bob@a.test"))
        .unwrap();
    let ciphertext = encrypt_update(
        &bob,
        &alice_pk,
        &bob_link.host_contact_id,
        &old_card,
        &new_card,
    );

    let bob_token = token_hex(&compute_mailbox_token(
        bob_link.shared_secret.as_bytes(),
        current_day_epoch(),
    ));

    let blobs = vec![("blob-1".to_string(), bob_token, ciphertext)];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].token_resolved, "token must resolve to Bob");
    assert!(outcomes[0].decrypted, "blob must decrypt via the fast path");

    // Card on Alice's side reflects Bob's update.
    let bob_at_alice = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let has_email = bob_at_alice
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == "Email" && f.value() == "bob@a.test");
    assert!(has_email, "Bob's card update must be applied");
}

// @scenario: receive_phase :: blob with empty mailbox_token is dropped
/// After Step 2 of the receive-phase-token-attribution plan, blobs
/// arriving without an attributed token can no longer be routed — every
/// in-spec relay populates the field. ACK as undecryptable and move on.
// @internal
#[test]
fn test_receive_drops_blob_when_token_missing() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(FieldType::Email, "Email", "bob@b.test"))
        .unwrap();
    let ciphertext = encrypt_update(
        &bob,
        &alice_pk,
        &bob_link.host_contact_id,
        &old_card,
        &new_card,
    );

    // Snapshot Bob's ratchet state BEFORE — drop path must not advance it.
    let (ratchet_before, _) = alice
        .storage()
        .load_ratchet_state(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_before_bytes = serde_json::to_vec(&ratchet_before.serialize()).unwrap();

    let blobs = vec![("blob-2".to_string(), String::new(), ciphertext)];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(!outcomes[0].token_resolved, "empty token must not resolve");
    assert!(
        !outcomes[0].decrypted,
        "missing token: no fallback, blob is dropped"
    );

    // Bob's card must be untouched — there was no successful decrypt.
    let bob_at_alice = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let has_email = bob_at_alice
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == "Email");
    assert!(
        !has_email,
        "no fallback: Bob's card must NOT be updated when the token is missing"
    );

    // Ratchet state must not have advanced — drop path returns before
    // any ratchet load. A future refactor that splits the atomic txn
    // would silently regress without this assertion.
    let (ratchet_after, _) = alice
        .storage()
        .load_ratchet_state(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_after_bytes = serde_json::to_vec(&ratchet_after.serialize()).unwrap();
    assert_eq!(
        ratchet_before_bytes, ratchet_after_bytes,
        "drop path must not mutate ratchet state"
    );
}

// @scenario: receive_phase :: blob with unknown mailbox_token is dropped
/// A spoofed or random token (one we never registered) cannot resolve.
/// Without the fallback, the blob is dropped — no ratchet attempts, no
/// card update.
// @internal
#[test]
fn test_receive_drops_blob_when_token_unknown() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(FieldType::Email, "Email", "bob@c.test"))
        .unwrap();
    let ciphertext = encrypt_update(
        &bob,
        &alice_pk,
        &bob_link.host_contact_id,
        &old_card,
        &new_card,
    );

    let unknown_token = "ff".repeat(32);
    let blobs = vec![("blob-3".to_string(), unknown_token, ciphertext)];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(
        !outcomes[0].token_resolved,
        "unknown token must not resolve"
    );
    assert!(
        !outcomes[0].decrypted,
        "unknown token: no fallback, blob is dropped"
    );

    let bob_at_alice = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let has_email = bob_at_alice
        .card()
        .fields()
        .iter()
        .any(|f| f.label() == "Email");
    assert!(!has_email, "no fallback: Bob's card must remain unchanged");
}

// @scenario: receive_phase :: garbage payload with valid token is reported as not decrypted
// @internal
#[test]
fn test_receive_reports_undecryptable_blob() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    // Token resolves to Bob, but the ciphertext isn't a valid ratchet
    // message — `process_single_card_update` rejects it.
    let blobs = vec![(
        "blob-bad".to_string(),
        token_hex(&compute_mailbox_token(
            bob_link.shared_secret.as_bytes(),
            current_day_epoch(),
        )),
        b"not a real ratchet message".to_vec(),
    )];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(
        outcomes[0].token_resolved,
        "valid token must resolve, even if payload is garbage"
    );
    assert!(
        !outcomes[0].decrypted,
        "garbage payload must not be reported as decrypted"
    );
}

// @scenario: receive_phase :: fast path is the only path for in-spec input
/// Multi-contact regression test for Step 2 of
/// receive-phase-token-attribution. Sets up Alice with 6 exchanged
/// contacts; each sends a card update; one blob uses yesterday's token
/// (clock-skew tolerance). Every blob must decrypt via the fast path —
/// no fallback exists, so any failure here would indicate the fast path
/// doesn't cover an in-spec case.
///
/// Deterministic on purpose (small fixed contact set). A `proptest`
/// version covering N in 1..32 was considered but the current shape
/// already exercises both today/yesterday branches and multi-contact
/// resolution; converting buys little for the additional complexity.
// @internal
#[test]
fn test_receive_fast_path_handles_all_in_spec_input() {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    let labels = ["Bob", "Carol", "Dave", "Eve", "Frank", "Grace"];
    let peers: Vec<vauchi_core::Vauchi> = labels
        .iter()
        .map(|name| create_vauchi_with_identity(name))
        .collect();
    let links: Vec<LinkedPeer> = peers
        .iter()
        .zip(labels.iter())
        .map(|(peer, label)| link_contacts(&alice, peer, label))
        .collect();

    let day = current_day_epoch();
    assert!(day > 0, "test requires non-zero day epoch");

    let mut blobs: Vec<(String, String, Vec<u8>)> = Vec::new();
    for (i, (peer, link)) in peers.iter().zip(links.iter()).enumerate() {
        let old_card = ContactCard::new(labels[i]);
        let mut new_card = ContactCard::new(labels[i]);
        new_card
            .add_field(ContactField::new(
                FieldType::Email,
                "Email",
                &format!("{}@bulk.test", labels[i].to_lowercase()),
            ))
            .unwrap();
        let ciphertext =
            encrypt_update(peer, &alice_pk, &link.host_contact_id, &old_card, &new_card);

        // First peer uses yesterday's token (clock-skew tolerance).
        let token_day = if i == 0 { day - 1 } else { day };
        let token = token_hex(&compute_mailbox_token(
            link.shared_secret.as_bytes(),
            token_day,
        ));
        blobs.push((format!("blob-{i}"), token, ciphertext));
    }

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), labels.len());
    for (i, outcome) in outcomes.iter().enumerate() {
        assert!(
            outcome.decrypted,
            "blob {i} ({}) must decrypt via the fast path — Step 2 left no fallback",
            labels[i]
        );
    }

    // Every contact's card now has the expected Email field.
    for (i, link) in links.iter().enumerate() {
        let contact = alice
            .storage()
            .load_contact(&link.peer_contact_id)
            .unwrap()
            .unwrap();
        let expected = format!("{}@bulk.test", labels[i].to_lowercase());
        assert!(
            contact
                .card()
                .fields()
                .iter()
                .any(|f| f.value() == expected),
            "{}'s card should have {expected}",
            labels[i]
        );
    }
}

// @scenario: receive_phase :: replayed blob is rejected on second submission
/// CC-13 stateful: process the same `(token, ciphertext)` twice. First
/// invocation decrypts and applies the update; second invocation must
/// be rejected by `process_single_card_update`'s replay check
/// (`storage.is_replay_nonce`). Token still resolves both times — the
/// rejection is at the rules layer, not the routing layer.
// @internal
#[test]
fn test_receive_rejects_replayed_blob() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    new_card
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@replay.test",
        ))
        .unwrap();
    let ciphertext = encrypt_update(
        &bob,
        &alice_pk,
        &bob_link.host_contact_id,
        &old_card,
        &new_card,
    );
    let token = token_hex(&compute_mailbox_token(
        bob_link.shared_secret.as_bytes(),
        current_day_epoch(),
    ));

    let contacts = alice.storage().list_contacts().unwrap();

    // First submission — must succeed.
    let first = process_received_blobs(
        alice.identity().unwrap(),
        alice.storage(),
        &contacts,
        vec![(
            "blob-replay-1".to_string(),
            token.clone(),
            ciphertext.clone(),
        )],
    );
    assert!(first[0].decrypted, "first submission must decrypt");

    // Snapshot card and ratchet state after the first submission.
    let card_after_first = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    let (ratchet_after_first, _) = alice
        .storage()
        .load_ratchet_state(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_after_first_bytes = serde_json::to_vec(&ratchet_after_first.serialize()).unwrap();

    // Second submission of the SAME ciphertext under the SAME token.
    let second = process_received_blobs(
        alice.identity().unwrap(),
        alice.storage(),
        &contacts,
        vec![("blob-replay-2".to_string(), token, ciphertext)],
    );
    assert!(
        second[0].token_resolved,
        "replay still resolves to the same contact"
    );
    assert!(
        !second[0].decrypted,
        "second submission must be rejected (replay nonce or ratchet desync)"
    );

    // Card and ratchet state must NOT have changed on the second pass.
    let card_after_second = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    let (ratchet_after_second, _) = alice
        .storage()
        .load_ratchet_state(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_after_second_bytes = serde_json::to_vec(&ratchet_after_second.serialize()).unwrap();
    assert_eq!(
        serde_json::to_string(&card_after_first).unwrap(),
        serde_json::to_string(&card_after_second).unwrap(),
        "replay must not mutate card"
    );
    assert_eq!(
        ratchet_after_first_bytes, ratchet_after_second_bytes,
        "replay must not advance ratchet (atomic txn rollback)"
    );
}

// @scenario: receive_phase :: mixed batch yields per-blob outcomes in input order
/// Single batch with {success, drop-on-unknown-token, garbage-with-valid-token, success}.
/// Verifies per-index outcome correctness, that decrypted blobs apply
/// their cards, and that drop/reject blobs leave state untouched.
// @internal
#[test]
fn test_receive_mixed_batch_preserves_order() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let charlie = create_vauchi_with_identity("Charlie");

    let bob_link = link_contacts(&alice, &bob, "Bob");
    let charlie_link = link_contacts(&alice, &charlie, "Charlie");

    let alice_pk = *alice.identity().unwrap().signing_public_key();

    // Bob update — index 0, success.
    let mut bob_new = ContactCard::new("Bob");
    bob_new
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@mixed.test",
        ))
        .unwrap();
    let bob_ct = encrypt_update(
        &bob,
        &alice_pk,
        &bob_link.host_contact_id,
        &ContactCard::new("Bob"),
        &bob_new,
    );
    let bob_token = token_hex(&compute_mailbox_token(
        bob_link.shared_secret.as_bytes(),
        current_day_epoch(),
    ));

    // Charlie update for index 3 — also success.
    let mut charlie_new = ContactCard::new("Charlie");
    charlie_new
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "charlie@mixed.test",
        ))
        .unwrap();
    let charlie_ct = encrypt_update(
        &charlie,
        &alice_pk,
        &charlie_link.host_contact_id,
        &ContactCard::new("Charlie"),
        &charlie_new,
    );
    let charlie_token = token_hex(&compute_mailbox_token(
        charlie_link.shared_secret.as_bytes(),
        current_day_epoch(),
    ));

    let blobs = vec![
        ("idx-0-bob-ok".to_string(), bob_token, bob_ct),
        (
            "idx-1-unknown".to_string(),
            "ee".repeat(32),
            b"any garbage".to_vec(),
        ),
        (
            "idx-2-rejected".to_string(),
            // Use Bob's token but garbage payload — token resolves, payload rejected.
            token_hex(&compute_mailbox_token(
                bob_link.shared_secret.as_bytes(),
                current_day_epoch(),
            )),
            b"not a ratchet message".to_vec(),
        ),
        ("idx-3-charlie-ok".to_string(), charlie_token, charlie_ct),
    ];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 4, "one outcome per input blob");

    // Order is preserved.
    assert_eq!(outcomes[0].message_id, "idx-0-bob-ok");
    assert_eq!(outcomes[1].message_id, "idx-1-unknown");
    assert_eq!(outcomes[2].message_id, "idx-2-rejected");
    assert_eq!(outcomes[3].message_id, "idx-3-charlie-ok");

    // Per-index flags.
    assert!(
        outcomes[0].token_resolved && outcomes[0].decrypted,
        "idx 0: success"
    );
    assert!(
        !outcomes[1].token_resolved && !outcomes[1].decrypted,
        "idx 1: unknown token, dropped"
    );
    assert!(
        outcomes[2].token_resolved && !outcomes[2].decrypted,
        "idx 2: token resolved, payload rejected"
    );
    assert!(
        outcomes[3].token_resolved && outcomes[3].decrypted,
        "idx 3: success"
    );

    // Bob's card has the Email; Charlie's card too.
    let bob_at_alice = alice
        .storage()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    assert!(
        bob_at_alice
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "bob@mixed.test"),
        "Bob's card must reflect idx-0 update"
    );
    let charlie_at_alice = alice
        .storage()
        .load_contact(&charlie_link.peer_contact_id)
        .unwrap()
        .unwrap();
    assert!(
        charlie_at_alice
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "charlie@mixed.test"),
        "Charlie's card must reflect idx-3 update"
    );
}
