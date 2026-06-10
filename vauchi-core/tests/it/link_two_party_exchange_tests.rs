// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Two-party link-mode exchange end-to-end (ADR-050 Phase 2, T5b / G4).
//!
//! The payoff of the symmetric exchange: a deep-link contact is no longer a
//! frozen import but a **live, updatable** `Exchanged` contact. Two real
//! `Vauchi` instances each deposit a v2 bootstrap, complete via the
//! production `complete_link_exchange` path, and end up with matching
//! `shared_key`s, opposite Double Ratchet roles, and a working update
//! channel — a card change on one side propagates to the other through the
//! ratchet, which a v1 import could never do.
//!
//! Record: `_private/docs/problems/2026-06-03-link-symmetric-exchange-wiring/`.
//! Feature: sync_updates.feature

use crate::common;

use common::helpers::create_vauchi_with_card;
use vauchi_core::contact::statistics::compute_statistics;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::link_mode::serialize_card_payload_v2;
use vauchi_core::{ContactField, ExchangeTransport, FieldType, Vauchi};

/// Build the v2 bootstrap a side deposits, embedding its real own card so a
/// later update has a meaningful baseline. Returns the payload, the retained
/// X3DH keypair (kept for completion), and the side's identity key.
fn deposit_v2(v: &Vauchi, relay_url: &str) -> (Vec<u8>, X3DHKeyPair, [u8; 32]) {
    let identity = v.identity().expect("identity exists");
    let identity_pubkey = *identity.signing_public_key();
    let x3dh = X3DHKeyPair::generate();
    let card = v.own_card().expect("own_card ok").expect("own card exists");
    let payload = serialize_card_payload_v2(
        &identity_pubkey,
        identity.signing_keypair(),
        x3dh.public_key(),
        relay_url,
        &card,
    );
    (payload, x3dh, identity_pubkey)
}

// @scenario: sync_updates :: Update propagates to link-exchanged contacts
#[test]
fn link_exchange_yields_live_updatable_contacts_on_both_sides() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@email.com")]);

    let (alice_payload, alice_x3dh, alice_id) = deposit_v2(&alice, "https://relay.alice.example");
    let (bob_payload, bob_x3dh, bob_id) = deposit_v2(&bob, "https://relay.bob.example");

    // Each side completes the peer's bootstrap via the real T5b path.
    let bob_in_alice = alice
        .complete_link_exchange(&bob_payload, &alice_x3dh)
        .expect("alice completes bob's bootstrap");
    let alice_in_bob = bob
        .complete_link_exchange(&alice_payload, &bob_x3dh)
        .expect("bob completes alice's bootstrap");

    // relay routing — not a frozen import.
    for (v, peer_id, peer_relay) in [
        (&alice, &bob_in_alice, "https://relay.bob.example"),
        (&bob, &alice_in_bob, "https://relay.alice.example"),
    ] {
        let c = v.get_contact(peer_id).unwrap().unwrap();
        assert!(
            c.kind().exchanged_data().is_some(),
            "must be a live Exchange, not a frozen import",
        );
        assert_eq!(c.exchange_transport(), Some(ExchangeTransport::Link));
        assert_eq!(c.relay_url(), Some(peer_relay));
    }

    let alices_bob = alice.get_contact(&bob_in_alice).unwrap().unwrap();
    let bobs_alice = bob.get_contact(&alice_in_bob).unwrap().unwrap();
    assert_eq!(
        alices_bob.shared_key().unwrap().as_bytes(),
        bobs_alice.shared_key().unwrap().as_bytes(),
        "both sides must derive the same symmetric link shared key",
    );

    // Each side counts exactly one Link exchange in its statistics breakdown.
    for v in [&alice, &bob] {
        let stats = compute_statistics(&v.list_contacts().unwrap(), 0);
        assert_eq!(
            stats
                .exchange_method_breakdown
                .get(&ExchangeTransport::Link)
                .copied(),
            Some(1),
            "the link contact is counted as a Link exchange",
        );
    }

    // A card update propagates over the new channel. The Double Ratchet
    // initiator (smaller identity key) must speak first — the responder has
    // no sending chain until it receives the initiator's first message.
    let alice_is_initiator = alice_id < bob_id;
    let (sender, receiver, recipient_in_sender, sender_in_receiver) = if alice_is_initiator {
        (&alice, &bob, &bob_in_alice, &alice_in_bob)
    } else {
        (&bob, &alice, &alice_in_bob, &bob_in_alice)
    };

    let old_card = sender.own_card().unwrap().unwrap();
    sender
        .add_own_field(ContactField::new(
            FieldType::Website,
            "blog",
            "https://sender.example",
            0,
        ))
        .unwrap();
    let new_card = sender.own_card().unwrap().unwrap();

    let encrypted = sender
        .prepare_card_update_for_contact(recipient_in_sender, &old_card, &new_card)
        .expect("sender prepares the card update over the link channel");
    let changed = receiver
        .process_card_update(sender_in_receiver, &encrypted)
        .expect("receiver applies the card update over the link channel");
    assert!(
        !changed.is_empty(),
        "the update must report at least one changed field",
    );

    // The receiver's copy of the sender's card reflects the new field — proof
    // the Link contact is live/updatable, which a v1 import never was.
    let senders_card_at_receiver = receiver.get_contact(sender_in_receiver).unwrap().unwrap();
    let blog = senders_card_at_receiver
        .card()
        .fields()
        .iter()
        .find(|f| f.label() == "blog")
        .expect("the new field must arrive over the link update channel");
    assert_eq!(blog.value(), "https://sender.example");
}
