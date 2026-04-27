// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Receive-phase routing integration tests.
//!
//! Exercises `process_received_blobs` end-to-end:
//! - mailbox-token attribution → O(1) fast path
//! - missing/unknown attribution → brute-force fallback
//! - card delta is applied to the resolved contact
//!
//! Traces to: `_private/docs/problems/2026-04-27-sync-receive-quadratic-contacts/`
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
/// receive loop uses the fast path and never touches Charlie's ratchet.
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
    assert!(outcomes[0].decrypted, "blob must decrypt");
    assert!(
        outcomes[0].via_token,
        "in-spec input MUST use the mailbox-token fast path, never the brute-force fallback"
    );

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

// @scenario: receive_phase :: missing attribution falls back to brute-force
/// Older relays don't emit `mailbox_token`. The blob arrives with an
/// empty token; the receive loop must still resolve via brute-force.
// @internal
#[test]
fn test_receive_falls_back_when_token_missing() {
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

    // Empty mailbox_token simulates the legacy-relay deserialised default.
    let blobs = vec![("blob-2".to_string(), String::new(), ciphertext)];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].decrypted, "fallback must decrypt the blob");
    assert!(
        !outcomes[0].via_token,
        "missing attribution must mark via_token=false (fallback path)"
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
        .any(|f| f.label() == "Email" && f.value() == "bob@b.test");
    assert!(
        has_email,
        "Bob's card must still be updated via the fallback path"
    );
}

// @scenario: receive_phase :: unknown attribution falls back to brute-force
/// Spoofed/random mailbox token shouldn't crash the loop — falls back
/// to brute-force, which still resolves to the legitimate sender.
// @internal
#[test]
fn test_receive_falls_back_when_token_unknown() {
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

    // Random unknown token — neither today's nor yesterday's contact token.
    let unknown_token = "ff".repeat(32);
    let blobs = vec![("blob-3".to_string(), unknown_token, ciphertext)];

    let contacts = alice.storage().list_contacts().unwrap();
    let outcomes =
        process_received_blobs(alice.identity().unwrap(), alice.storage(), &contacts, blobs);

    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].decrypted, "brute-force must still resolve");
    assert!(
        !outcomes[0].via_token,
        "unknown token must miss the fast path"
    );
}

// @scenario: receive_phase :: undecryptable blob is reported as not decrypted
// @internal
#[test]
fn test_receive_reports_undecryptable_blob() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");
    let bob_link = link_contacts(&alice, &bob, "Bob");

    // Garbage ciphertext — no contact's ratchet decrypts.
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
        !outcomes[0].decrypted,
        "garbage payload must not be reported as decrypted"
    );
    assert!(
        !outcomes[0].via_token,
        "via_token implies a successful fast-path decrypt — must remain false on failure"
    );
}
