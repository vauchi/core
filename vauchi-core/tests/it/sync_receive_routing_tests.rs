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
use vauchi_core::api::VauchiEvent;
use vauchi_core::api::vauchi::{incoming_update_events, process_received_blobs};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::cek::ContentEncryptionKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::reciprocity::{ConfirmationChannel, Reciprocity};
use vauchi_core::exchange::reciprocity_tokens::derive_confirmation_tokens;
use vauchi_core::network::mailbox_token::{compute_mailbox_token, current_day_epoch, token_hex};
use vauchi_core::sync::delta::{
    CardDelta, CekWrappedPayload, ReciprocityConfirmPayload, VersionedPayload,
};

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
        Contact::from_exchange(peer_pk, ContactCard::new(label), shared_secret.clone(), 0);
    let peer_contact_id = peer_at_host.id().to_string();
    host.add_contact(peer_at_host).unwrap();

    let host_at_peer =
        Contact::from_exchange(host_pk, ContactCard::new("host"), shared_secret.clone(), 0);
    let host_contact_id = host_at_peer.id().to_string();
    peer.add_contact(host_at_peer).unwrap();

    let host_dh = X3DHKeyPair::generate();
    let peer_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *host_dh.public_key()).unwrap();
    let host_ratchet = DoubleRatchetState::initialize_responder(&shared_secret, host_dh);

    host.storage()
        .ratchets()
        .save_ratchet_state(&peer_contact_id, &host_ratchet, false)
        .unwrap();
    peer.storage()
        .ratchets()
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
    let mut delta = CardDelta::compute(old_card, new_card, 0);
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
        .ratchets()
        .load_ratchet_state(recipient_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = sender_ratchet.encrypt(&payload).unwrap();
    sender
        .storage()
        .ratchets()
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
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@a.test",
            0,
        ))
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
        &alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
    ));

    let blobs = vec![("blob-1".to_string(), bob_token, ciphertext)];

    let contacts = alice.storage().contacts().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].token_resolved, "token must resolve to Bob");
    assert!(outcomes[0].decrypted, "blob must decrypt via the fast path");

    // The applied blob carries the resolved contact id, and the receive
    // phase turns it into an IncomingUpdate so the frontend invalidates and
    // re-renders the contacts list (regression: 2026-06-30
    // S7-synced-tile-not-rendered; affected_screens maps IncomingUpdate ->
    // ["contacts", "contact_detail"]).
    assert_eq!(
        outcomes[0].contact_id.as_deref(),
        Some(bob_link.peer_contact_id.as_str()),
        "applied blob must carry the resolved contact id"
    );
    let events = incoming_update_events(&outcomes);
    assert_eq!(events.len(), 1, "one applied blob -> one IncomingUpdate");
    match &events[0] {
        VauchiEvent::IncomingUpdate { contact_id } => {
            assert_eq!(contact_id, &bob_link.peer_contact_id);
        }
        other => panic!("expected IncomingUpdate, got {other:?}"),
    }

    // Card on Alice's side reflects Bob's update.
    let bob_at_alice = alice
        .storage()
        .contacts()
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
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@b.test",
            0,
        ))
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
        .ratchets()
        .load_ratchet_state(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_before_bytes = serde_json::to_vec(&ratchet_before.serialize()).unwrap();

    let blobs = vec![("blob-2".to_string(), String::new(), ciphertext)];

    let contacts = alice.storage().contacts().list_contacts().unwrap();
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
        .contacts()
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
        .ratchets()
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
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "bob@c.test",
            0,
        ))
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

    let contacts = alice.storage().contacts().list_contacts().unwrap();
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
        .contacts()
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
            alice.identity().unwrap().signing_public_key(),
            current_day_epoch(alice.storage().clock().unix_seconds()),
        )),
        b"not a real ratchet message".to_vec(),
    )];

    let contacts = alice.storage().contacts().list_contacts().unwrap();
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

    let day = current_day_epoch(alice.storage().clock().unix_seconds());
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
                0,
            ))
            .unwrap();
        let ciphertext =
            encrypt_update(peer, &alice_pk, &link.host_contact_id, &old_card, &new_card);

        // First peer uses yesterday's token (clock-skew tolerance).
        let token_day = if i == 0 { day - 1 } else { day };
        let token = token_hex(&compute_mailbox_token(
            link.shared_secret.as_bytes(),
            &alice_pk,
            token_day,
        ));
        blobs.push((format!("blob-{i}"), token, ciphertext));
    }

    let contacts = alice.storage().contacts().list_contacts().unwrap();
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
            .contacts()
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
            0,
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
        &alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
    ));

    let contacts = alice.storage().contacts().list_contacts().unwrap();

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
        .contacts()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    let (ratchet_after_first, _) = alice
        .storage()
        .ratchets()
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
        .contacts()
        .load_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap()
        .card()
        .clone();
    let (ratchet_after_second, _) = alice
        .storage()
        .ratchets()
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
            0,
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
        &alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
    ));

    // Charlie update for index 3 — also success.
    let mut charlie_new = ContactCard::new("Charlie");
    charlie_new
        .add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "charlie@mixed.test",
            0,
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
        &alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
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
                &alice_pk,
                current_day_epoch(alice.storage().clock().unix_seconds()),
            )),
            b"not a ratchet message".to_vec(),
        ),
        ("idx-3-charlie-ok".to_string(), charlie_token, charlie_ct),
    ];

    let contacts = alice.storage().contacts().list_contacts().unwrap();
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

    let bob_at_alice = alice
        .storage()
        .contacts()
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
        .contacts()
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

/// Encrypt a P3 reciprocity confirmation `sender` sends the recipient: a signed
/// `ReciprocityConfirmPayload` carrying `token`, ratchet-encrypted like a card
/// update (same wire envelope).
fn encrypt_reciprocity_confirm(
    sender: &vauchi_core::Vauchi,
    recipient_signing_pk: &[u8; 32],
    recipient_contact_id: &str,
    token: [u8; 32],
) -> Vec<u8> {
    let sender_identity = sender.identity().unwrap();
    let payload = ReciprocityConfirmPayload::new(token, sender_identity, recipient_signing_pk);
    let encoded = VersionedPayload::encode_reciprocity(&payload);

    let (mut sender_ratchet, is_init) = sender
        .storage()
        .ratchets()
        .load_ratchet_state(recipient_contact_id)
        .unwrap()
        .unwrap();
    let ratchet_msg = sender_ratchet.encrypt(&encoded).unwrap();
    sender
        .storage()
        .ratchets()
        .save_ratchet_state(recipient_contact_id, &sender_ratchet, is_init)
        .unwrap();
    serde_json::to_vec(&ratchet_msg).unwrap()
}

/// Mailbox token Bob would post under so Alice attributes the blob to him.
fn bob_mailbox_token(
    alice: &vauchi_core::Vauchi,
    link: &LinkedPeer,
    alice_pk: &[u8; 32],
) -> String {
    token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        alice_pk,
        current_day_epoch(alice.storage().clock().unix_seconds()),
    ))
}

// CC-03: a valid relay-sync reciprocity confirmation resolves the contact to
// Confirmed via RelaySync (P3 Slice A end-to-end: verify sig → derive
// expected_their_token → match → confirm).
// @scenario: receive_phase :: relay-sync reciprocity confirmation resolves Confirmed
// @internal
#[test]
fn reciprocity_confirm_resolves_contact_to_confirmed() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();

    // Bob sends HIS our_token — cross-matches Alice's derived expected_their_token.
    let (bob_our_token, _) =
        derive_confirmation_tokens(bob_link.shared_secret.as_bytes(), &bob_pk, &alice_pk);

    let ciphertext =
        encrypt_reciprocity_confirm(&bob, &alice_pk, &bob_link.host_contact_id, *bob_our_token);
    let blobs = vec![(
        "confirm-1".to_string(),
        bob_mailbox_token(&alice, &bob_link, &alice_pk),
        ciphertext,
    )];
    let contacts = alice.storage().contacts().list_contacts().unwrap();

    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);
    assert!(
        outcomes[0].decrypted,
        "the confirmation must decrypt + verify"
    );

    let now = alice.storage().clock().unix_seconds();
    let contact = alice
        .get_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        contact.reciprocity(now),
        Reciprocity::Confirmed,
        "a matching signed confirmation resolves reciprocity to Confirmed"
    );
    assert_eq!(
        contact.confirmation_channel(),
        Some(ConfirmationChannel::RelaySync),
        "confirmation channel is recorded as relay sync"
    );
}

// CC-14: a confirmation carrying the WRONG token — even with a valid signature
// (the sender signs whatever token) — must NOT confirm. G1: the relay cannot
// manufacture a false Confirmed.
// @scenario: receive_phase :: wrong-token reciprocity confirmation never confirms
// @internal
#[test]
fn reciprocity_confirm_wrong_token_never_confirms() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    // A validly-signed confirmation over a bogus token (not Bob's our_token).
    let ciphertext =
        encrypt_reciprocity_confirm(&bob, &alice_pk, &bob_link.host_contact_id, [0xFFu8; 32]);
    let blobs = vec![(
        "confirm-bad".to_string(),
        bob_mailbox_token(&alice, &bob_link, &alice_pk),
        ciphertext,
    )];
    let contacts = alice.storage().contacts().list_contacts().unwrap();

    let _ = process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    let now = alice.storage().clock().unix_seconds();
    let contact = alice
        .get_contact(&bob_link.peer_contact_id)
        .unwrap()
        .unwrap();
    assert_ne!(
        contact.reciprocity(now),
        Reciprocity::Confirmed,
        "a wrong-token confirmation must never resolve to Confirmed"
    );
}

// CC-03 end-to-end (Slice B → Slice A): a Pending contact's queued confirmation,
// delivered to the peer, resolves the peer to Confirmed — the full relay-sync
// loop. Bob (ratchet initiator) sends; Alice (responder) receives.
// @scenario: receive_phase :: queued reciprocity confirmation closes the loop
// @internal
#[test]
fn queued_reciprocity_confirmation_resolves_peer_to_confirmed() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");
    let now = alice.storage().clock().unix_seconds();
    let alice_pk = *alice.identity().unwrap().signing_public_key();

    // Bob's contact-of-Alice is a Pending confirmable exchange. Rebuild it with a
    // recent exchange timestamp (link_contacts uses 0, which the 7-day read-time
    // timer would decay to Unreciprocated); the ratchet is keyed by contact id so
    // it survives the upsert.
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

    // Slice B: Bob queues one confirmation for Alice.
    let queued = bob.queue_reciprocity_confirmations().unwrap();
    assert_eq!(
        queued, 1,
        "one Pending confirmable contact -> one queued confirmation"
    );

    // Extract the queued payload (what the send phase would post) and deliver it
    // to Alice via the receive path, under Alice's mailbox token.
    let updates = bob.storage().pending().get_all_pending_updates().unwrap();
    assert_eq!(updates.len(), 1, "exactly one pending update queued");
    let payload = updates[0].payload.clone();
    let alice_token = token_hex(&compute_mailbox_token(
        link.shared_secret.as_bytes(),
        &alice_pk,
        current_day_epoch(now),
    ));
    let blobs = vec![("confirm-loop".to_string(), alice_token, payload)];
    let alice_contacts = alice.storage().contacts().list_contacts().unwrap();
    let _ = process_received_blobs(
        alice.identity().unwrap(),
        alice.storage(),
        &alice_contacts,
        blobs,
    );

    let alice_of_bob = alice.get_contact(&link.peer_contact_id).unwrap().unwrap();
    assert_eq!(
        alice_of_bob.reciprocity(now),
        Reciprocity::Confirmed,
        "the delivered confirmation resolves the peer to Confirmed"
    );
    assert_eq!(
        alice_of_bob.confirmation_channel(),
        Some(ConfirmationChannel::RelaySync)
    );
}

// The send gate: a Confirmed contact is not re-sent (convergence — the loop
// terminates once both sides agree).
// @scenario: receive_phase :: confirmed contacts are not re-confirmed
// @internal
#[test]
fn queue_skips_already_confirmed_contacts() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let link = link_contacts(&alice, &bob, "Bob");

    let mut bob_of_alice = bob.get_contact(&link.host_contact_id).unwrap().unwrap();
    bob_of_alice.set_reciprocity(Reciprocity::Confirmed);
    bob.storage()
        .contacts()
        .save_contact(&bob_of_alice)
        .unwrap();

    assert_eq!(
        bob.queue_reciprocity_confirmations().unwrap(),
        0,
        "a Confirmed contact must not be re-sent"
    );
}
