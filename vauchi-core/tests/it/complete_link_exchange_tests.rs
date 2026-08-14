// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Vauchi::complete_link_exchange` — ADR-050 Phase 2 (T5b).
//!
//! Where `import_received_link_card` produces a frozen **import**, the v2
//! symmetric bootstrap produces a live, updatable **Exchanged** contact:
//! both sides derive the same `shared_key` (commutative DH) and initialize
//! a Double Ratchet with a deterministic role — the **smaller identity key
//! is the initiator**, the same rule as in-person exchange
//! (`ExchangeSession::build_exchange_ratchet`). A v1 payload has no
//! exchange key, so it falls back to the import path.
//!
//! Record: `_private/docs/problems/2026-06-03-link-symmetric-exchange-wiring/`.

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_mode::{serialize_card_payload, serialize_card_payload_v2};
use vauchi_core::{ExchangeTransport, ImportSource, Vauchi, VauchiError};

fn vauchi_with_identity(name: &str) -> Vauchi {
    let mut v = Vauchi::in_memory().expect("in-memory vauchi");
    v.create_identity(name).expect("create identity");
    v
}

/// Build the v2 bootstrap a side would deposit, returning the payload, the
/// retained X3DH keypair (kept for completion), and the side's identity key.
fn build_v2(v: &Vauchi, relay_url: &str) -> (Vec<u8>, X3DHKeyPair, [u8; 32]) {
    let identity = v.identity().expect("identity exists");
    let identity_pubkey = *identity.signing_public_key();
    let x3dh = X3DHKeyPair::generate();
    let card = ContactCard::new(identity.display_name());
    let payload = serialize_card_payload_v2(
        &identity_pubkey,
        identity.signing_keypair(),
        x3dh.public_key(),
        relay_url,
        &card,
    );
    (payload, x3dh, identity_pubkey)
}

// @internal
#[test]
fn v2_two_party_completes_to_a_live_exchanged_link_contact_with_symmetric_key() {
    let alice = vauchi_with_identity("Alice");
    let bob = vauchi_with_identity("Bob");

    let (alice_payload, alice_x3dh, alice_id_pub) = build_v2(&alice, "https://relay.alice.example");
    let (bob_payload, bob_x3dh, bob_id_pub) = build_v2(&bob, "https://relay.bob.example");

    // Each side completes the *peer's* bootstrap with its own retained key.
    let bob_contact_id = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect("alice completes bob's bootstrap");
    let alice_contact_id = bob
        .complete_link_exchange(&alice_payload, &bob_x3dh)
        .expect("bob completes alice's bootstrap");

    // Both produced an Exchanged (live, updatable) contact over Link, with
    // the peer's relay routing — not a frozen import.
    let alices_bob = alice
        .get_contact(&bob_contact_id)
        .expect("get ok")
        .expect("alice has a contact for bob");
    let bobs_alice = bob
        .get_contact(&alice_contact_id)
        .expect("get ok")
        .expect("bob has a contact for alice");

    assert!(
        alices_bob.kind().exchanged_data().is_some(),
        "link v2 must be Exchanged (live channel), not Imported",
    );
    assert_eq!(
        alices_bob.exchange_transport(),
        Some(ExchangeTransport::Link)
    );
    assert_eq!(
        alices_bob.relay_url(),
        Some("https://relay.bob.example"),
        "the contact carries the peer's relay routing for the update channel",
    );

    // The shared key is symmetric — both sides derived the same one.
    assert_eq!(
        alices_bob
            .shared_key()
            .expect("alice's shared key")
            .as_bytes(),
        bobs_alice
            .shared_key()
            .expect("bob's shared key")
            .as_bytes(),
        "both sides must derive the same link shared key",
    );

    // Deterministic role: the smaller identity key is the ratchet initiator
    // (decision (b); matches ExchangeSession::build_exchange_ratchet).
    let alice_is_initiator_by_rule = alice_id_pub < bob_id_pub;

    // ...and only the initiator can speak first (the responder has no
    // sending chain until it receives the initiator's first message).
    let alice_can_send_first = alice
        .get_ratchet_state(&bob_contact_id)
        .expect("ratchet load ok")
        .expect("alice has a ratchet")
        .encrypt(b"probe")
        .is_ok();
    let bob_can_send_first = bob
        .get_ratchet_state(&alice_contact_id)
        .expect("ratchet load ok")
        .expect("bob has a ratchet")
        .encrypt(b"probe")
        .is_ok();

    assert_ne!(
        alice_can_send_first, bob_can_send_first,
        "exactly one side must be the ratchet initiator",
    );
    assert_eq!(
        alice_can_send_first, alice_is_initiator_by_rule,
        "the side that can speak first must be the smaller-identity-key side",
    );

    // Full Double Ratchet round-trip across the two stores (fresh fetches,
    // since the probe above advanced detached copies).
    let (mut initiator, mut responder) = if alice_is_initiator_by_rule {
        (
            alice.get_ratchet_state(&bob_contact_id).unwrap().unwrap(),
            bob.get_ratchet_state(&alice_contact_id).unwrap().unwrap(),
        )
    } else {
        (
            bob.get_ratchet_state(&alice_contact_id).unwrap().unwrap(),
            alice.get_ratchet_state(&bob_contact_id).unwrap().unwrap(),
        )
    };

    let hello = b"hello from the initiator";
    let ct = initiator.encrypt(hello).expect("initiator encrypts");
    let pt = responder
        .decrypt(&ct)
        .expect("responder decrypts the initiator's first message");
    assert_eq!(pt, hello, "initiator->responder plaintext must survive");

    let reply = b"reply from the responder";
    let ct = responder.encrypt(reply).expect("responder encrypts reply");
    let pt = initiator
        .decrypt(&ct)
        .expect("initiator decrypts the responder's reply");
    assert_eq!(pt, reply, "responder->initiator plaintext must survive");
}

// @internal
#[test]
fn v2_completion_is_idempotent_and_keeps_the_existing_channel() {
    let alice = vauchi_with_identity("Alice");
    let bob = vauchi_with_identity("Bob");
    let (bob_payload, _bob_x3dh, _) = build_v2(&bob, "https://relay.bob.example");
    let (_, alice_x3dh, _) = build_v2(&alice, "https://relay.alice.example");

    let id1 = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect("first completion");
    let ratchet_before = alice
        .get_ratchet_state(&id1)
        .expect("ratchet load ok")
        .expect("ratchet exists");

    // Re-receiving the same peer returns the existing contact and must NOT
    // re-key the channel (a fresh ratchet would desync an in-flight session).
    let id2 = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect("second completion");
    assert_eq!(id1, id2, "re-receiving the same peer returns the same id");
    assert_eq!(
        alice.list_contacts().expect("list").len(),
        1,
        "idempotent — the same peer must not duplicate",
    );

    let ratchet_after = alice
        .get_ratchet_state(&id1)
        .expect("ratchet load ok")
        .expect("ratchet still exists");
    assert_eq!(
        ratchet_before.our_public_key(),
        ratchet_after.our_public_key(),
        "the ratchet must be preserved, not re-keyed, on re-exchange",
    );
}

// @internal
#[test]
fn v1_payload_falls_back_to_a_frozen_import() {
    let alice = vauchi_with_identity("Alice");
    let (_, alice_x3dh, _) = build_v2(&alice, "https://relay.alice.example");

    // A legacy v1 payload carries no exchange key.
    let peer = vauchi_with_identity("LegacyBob");
    let peer_identity = peer.identity().unwrap();
    let card = ContactCard::new("LegacyBob");
    let v1_payload = serialize_card_payload(peer_identity.signing_public_key(), &card);

    let id = alice
        .complete_link_exchange(&v1_payload, &alice_x3dh)
        .expect("v1 completes via the import fallback");

    let contact = alice.get_contact(&id).unwrap().unwrap();
    let imported = contact
        .kind()
        .imported_data()
        .expect("a v1 link payload must yield a frozen Import, not an Exchange");
    assert_eq!(imported.source, ImportSource::LinkExchange);
}

// @internal
#[test]
fn v2_rejects_completing_our_own_bootstrap() {
    let alice = vauchi_with_identity("Alice");
    let (alice_payload, alice_x3dh, _) = build_v2(&alice, "https://relay.alice.example");

    // Receiving our own signed bootstrap is degenerate (identity == ours);
    // it must be rejected, never written as a self-contact.
    let err = alice
        .complete_link_exchange(&alice_payload, &alice_x3dh)
        .expect_err("completing our own bootstrap must fail");
    // The engine picks the user-facing consequence from the variant, so an
    // untyped rejection here becomes "the card could not be decrypted" — the
    // message that misdirected
    // 2026-08-14-link-exchange-responder-cannot-decrypt-the-card.
    assert!(
        matches!(
            err,
            VauchiError::Exchange(vauchi_core::exchange::ExchangeError::SelfExchange)
        ),
        "a self-exchange must be typed so the engine can name it, got {err:?}"
    );
    assert_eq!(
        alice.list_contacts().expect("list").len(),
        0,
        "a self-exchange must not create a contact",
    );
}

// @scenario: link_exchange :: a full contact list names the limit, not a crypto failure
#[test]
fn v2_completion_at_the_contact_limit_reports_the_limit() {
    let alice = vauchi_with_identity("Alice");
    let bob = vauchi_with_identity("Bob");
    let (bob_payload, _, _) = build_v2(&bob, "https://relay.bob.example");
    let (_, alice_x3dh, _) = build_v2(&alice, "https://relay.alice.example");

    alice
        .storage()
        .contacts()
        .set_contact_limit(0)
        .expect("set contact limit");

    let err = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect_err("completing past the contact limit must fail");
    assert!(
        matches!(err, VauchiError::ContactLimitReached(0)),
        "the limit must be typed so the user is told to free a slot rather \
         than to retry, got {err:?}"
    );
}
