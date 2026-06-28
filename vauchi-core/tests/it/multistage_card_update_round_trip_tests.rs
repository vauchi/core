// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: a card UPDATE round-trips over a REAL multi-stage (mobile QR)
//! exchange — the exact path iOS/Android use.
//!
//! `multistage_ratchet_roundtrip_tests` proves the multi-stage ratchet
//! round-trips DIRECT messages on the in-memory ratchet objects. This pins the
//! layer the device sync actually uses but that file does not: the ratchet is
//! **persisted then reloaded** through `save_exchanged_contact` (as on a device
//! between the exchange and the later sync), and the payload crosses the full
//! **card-update pipeline** (`prepare_card_update_for_contact` →
//! `process_card_update`: CEK wrap, signature binding, delta/replay).
//!
//! The mobile QR exchange routes through `MultiStageSession::build_exchange_ratchet`
//! (`multi_stage_exchange.rs:373`), keyed off the multistage `transport_key`
//! (HKDF `vauchi-multistage-v1`), NOT the X3DH `ExchangeSession` path. The
//! on-device symptom (`sync.receive_phase received=0 rejected=N` — blobs
//! delivered + token-routed but never decrypt) reproduces here with zero relay
//! and zero CLI if this layer is the cause.
//!
//! Problem record: 2026-06-28-sync-delivery-sent-not-received (step 2).
//! Feature: features/sync_updates.feature, features/contact_exchange.feature @multi-stage

use crate::common;

use common::helpers::create_vauchi_with_card;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::{Contact, FieldType, SymmetricKey};

/// Drive two sessions through a full multi-stage exchange to `Finalized`
/// (mirrors `multistage_ratchet_roundtrip_tests::drive_to_finalized`).
fn drive_to_finalized(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
) -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    for _ in 0..100 {
        let alice_qr = alice.get_display_qr();
        let bob_qr = bob.get_display_qr();
        if let Some(aq) = &alice_qr {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bob_qr {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(
        matches!(alice.get_state(), ProtocolState::Finalized),
        "alice finalized"
    );
    assert!(
        matches!(bob.get_state(), ProtocolState::Finalized),
        "bob finalized"
    );
    (alice, bob)
}

// @scenario: sync_updates :: A card update propagates after a multi-stage exchange
#[test]
fn multistage_in_person_exchange_card_update_round_trips() {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@old.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // ── Real multi-stage exchange to Finalized; the ratchet is keyed off the
    //    transport ephemerals, so the card bytes here are immaterial.
    let (alice_session, bob_session) =
        drive_to_finalized(b"alice card".to_vec(), b"bob card".to_vec());
    let alice_tk = alice_session
        .get_transport_key()
        .expect("alice transport key");
    let bob_tk = bob_session.get_transport_key().expect("bob transport key");
    assert_eq!(alice_tk, bob_tk, "transport key must be symmetric");

    // The real identity keys drive the deterministic role decision so the
    // persisted contact ids (Contact::from_exchange) and the ratchet roles agree.
    let (alice_ratchet, alice_is_initiator) = alice_session
        .build_exchange_ratchet(&alice_pk, &bob_pk)
        .expect("alice ratchet builds");
    let (bob_ratchet, bob_is_initiator) = bob_session
        .build_exchange_ratchet(&bob_pk, &alice_pk)
        .expect("bob ratchet builds");
    assert_ne!(
        alice_is_initiator, bob_is_initiator,
        "exactly one side must be the initiator"
    );

    // ── Persist through the real save seam (serialise + reload on prepare/process).
    let bob_at_alice = Contact::from_exchange(
        bob_pk,
        bob_card.clone(),
        SymmetricKey::from_bytes(alice_tk),
        0,
    );
    let alice_at_bob = Contact::from_exchange(
        alice_pk,
        alice_card.clone(),
        SymmetricKey::from_bytes(bob_tk),
        0,
    );
    let bob_id = bob_at_alice.id().to_string();
    let alice_id = alice_at_bob.id().to_string();

    alice
        .save_exchanged_contact(&bob_at_alice, &alice_ratchet, alice_is_initiator)
        .unwrap();
    bob.save_exchanged_contact(&alice_at_bob, &bob_ratchet, bob_is_initiator)
        .unwrap();

    // ── Card update: the INITIATOR sends first (responder must receive once
    //    before it can send). This is the device "Alice edits her email" path.
    if alice_is_initiator {
        common::helpers::assert_card_update_round_trips(&alice, &bob, &bob_id, &alice_id);
    } else {
        common::helpers::assert_card_update_round_trips(&bob, &alice, &alice_id, &bob_id);
    }
}
