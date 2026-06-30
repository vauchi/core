// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Regression: a card UPDATE round-trips over a REAL multi-stage (mobile QR)
//! exchange — the exact path iOS/Android use — and the negative mechanism that
//! reproduces the device "received=0 rejected=N" symptom.
//!
//! `multistage_ratchet_roundtrip_tests` proves the multi-stage ratchet
//! round-trips DIRECT messages on the in-memory ratchet objects. This pins the
//! layer the device sync actually uses but that file does not: the ratchet is
//! **persisted then reloaded** through `save_exchanged_contact` (as on a device
//! between the exchange and the later sync), and the payload crosses the full
//! **card-update pipeline** (`seal_update` → `process_single_card_update`:
//! CEK wrap, signature binding, delta/replay).
//!
//! The mobile QR exchange routes through `MultiStageSession::build_exchange_ratchet`
//! (`multi_stage_exchange.rs:373`), keyed off the multistage `transport_key`
//! (HKDF `vauchi-multistage-v1`), NOT the X3DH `ExchangeSession` path.
//!
//! The positive test PASSES — core's pipeline is sound. The negative test pins
//! the device-bug MECHANISM: when the mobile completion's `build_exchange_ratchet`
//! returns `None` (silenced via `.ok()` at `multi_stage_exchange.rs:379-386`),
//! the contact is saved WITHOUT a ratchet via `update_contact`, and every later
//! update is rejected — exactly the on-device `received=0 rejected=N`.
//!
//! Problem record: 2026-06-28-sync-delivery-sent-not-received (step 2).
//! Feature: features/sync_updates.feature, features/contact_exchange.feature @multi-stage

use crate::common;

use common::helpers::{create_vauchi_with_card, create_vauchi_with_identity};
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::{
    Contact, ContactField, FieldType, SymmetricKey, Vauchi,
    api::{CardUpdateError, process_single_card_update},
};

/// Two Vauchis that have completed a real multi-stage exchange, with the
/// role-correct ratchet built for each side (not yet persisted).
struct ExchangedPair {
    alice: Vauchi,
    bob: Vauchi,
    alice_ratchet: DoubleRatchetState,
    alice_is_initiator: bool,
    bob_ratchet: DoubleRatchetState,
    bob_is_initiator: bool,
    bob_at_alice: Contact,
    alice_at_bob: Contact,
}

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

/// Run a real multi-stage exchange between two fresh Vauchis and build each
/// side's role-correct ratchet via the production `build_exchange_ratchet` seam.
fn multistage_exchanged_pair() -> ExchangedPair {
    let alice = create_vauchi_with_card("Alice", vec![(FieldType::Email, "work", "alice@old.com")]);
    let bob = create_vauchi_with_card("Bob", vec![(FieldType::Email, "personal", "bob@old.com")]);

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    // The ratchet is keyed off the transport ephemerals, so the card bytes
    // handed to the session here are immaterial.
    let (alice_session, bob_session) =
        drive_to_finalized(b"alice card".to_vec(), b"bob card".to_vec());
    let alice_tk = alice_session
        .get_transport_key()
        .expect("alice transport key");
    let bob_tk = bob_session.get_transport_key().expect("bob transport key");
    assert_eq!(alice_tk, bob_tk, "transport key must be symmetric");

    // Real identity keys drive the deterministic role decision so the persisted
    // contact ids and the ratchet roles agree.
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

    let bob_at_alice =
        Contact::from_exchange(bob_pk, bob_card, SymmetricKey::from_bytes(alice_tk), 0);
    let alice_at_bob =
        Contact::from_exchange(alice_pk, alice_card, SymmetricKey::from_bytes(bob_tk), 0);

    ExchangedPair {
        alice,
        bob,
        alice_ratchet,
        alice_is_initiator,
        bob_ratchet,
        bob_is_initiator,
        bob_at_alice,
        alice_at_bob,
    }
}

// @scenario: sync_updates :: A card update propagates after a multi-stage exchange
#[test]
fn multistage_in_person_exchange_card_update_round_trips() {
    let p = multistage_exchanged_pair();
    let bob_id = p.bob_at_alice.id().to_string();
    let alice_id = p.alice_at_bob.id().to_string();

    // ── Persist through the real save seam (serialise + reload on prepare/process).
    p.alice
        .save_exchanged_contact(&p.bob_at_alice, &p.alice_ratchet, p.alice_is_initiator)
        .unwrap();
    p.bob
        .save_exchanged_contact(&p.alice_at_bob, &p.bob_ratchet, p.bob_is_initiator)
        .unwrap();

    // ── The INITIATOR sends first (responder must receive once before it can
    //    send). This is the device "Alice edits her email" path.
    if p.alice_is_initiator {
        common::helpers::assert_card_update_round_trips(&p.alice, &p.bob, &bob_id, &alice_id);
    } else {
        common::helpers::assert_card_update_round_trips(&p.bob, &p.alice, &alice_id, &bob_id);
    }
}

/// Bidirectional: the initiator sends first (responder receives + applies),
/// THEN the responder replies and the initiator applies it. This is the
/// device "both peers added a field" case — the responder can only send after
/// it has received once, so the receive-then-send sync order must hold both
/// ways. Repro target for the bidirectional desync seen on hardware
/// (2026-06-28-sync-delivery-sent-not-received, "added mails on both").
// @scenario: sync_updates :: Bidirectional card updates after a multi-stage exchange
#[test]
fn multistage_bidirectional_card_updates_round_trip() {
    let p = multistage_exchanged_pair();
    let bob_id = p.bob_at_alice.id().to_string();
    let alice_id = p.alice_at_bob.id().to_string();

    p.alice
        .save_exchanged_contact(&p.bob_at_alice, &p.alice_ratchet, p.alice_is_initiator)
        .unwrap();
    p.bob
        .save_exchanged_contact(&p.alice_at_bob, &p.bob_ratchet, p.bob_is_initiator)
        .unwrap();

    let (initiator, responder, resp_id_at_init, init_id_at_resp) = if p.alice_is_initiator {
        (&p.alice, &p.bob, bob_id, alice_id)
    } else {
        (&p.bob, &p.alice, alice_id, bob_id)
    };

    // 1. Initiator → responder (establishes the responder's sending chain).
    common::helpers::assert_card_update_round_trips(
        initiator,
        responder,
        &resp_id_at_init,
        &init_id_at_resp,
    );
    // 2. Responder → initiator (the reply the device's bidirectional case needs).
    common::helpers::assert_card_update_round_trips(
        responder,
        initiator,
        &init_id_at_resp,
        &resp_id_at_init,
    );
}

/// MECHANISM: a contact persisted WITHOUT a ratchet — the silenced
/// `build_exchange_ratchet` None-path at mobile completion saves the contact
/// via `update_contact` instead of `save_exchanged_contact` — rejects every
/// incoming card update with `NotFound("ratchet state")`. In the receive phase
/// that is `token_resolved && !decrypted`, i.e. the device's `rejected=N`.
// @scenario: sync_updates :: A contact saved without a ratchet rejects card updates
#[test]
fn multistage_contact_without_ratchet_rejects_card_update() {
    let p = multistage_exchanged_pair();
    let bob_id = p.bob_at_alice.id().to_string();
    let alice_id = p.alice_at_bob.id().to_string();

    // Save the INITIATOR with its ratchet so it can SEND; save the RESPONDER
    // via the contact-only path (no ratchet) — the silenced None-path.
    let (sender, sender_old_value, recipient, recipient_id_at_sender, sender_id_at_recipient) =
        if p.alice_is_initiator {
            p.alice
                .save_exchanged_contact(&p.bob_at_alice, &p.alice_ratchet, true)
                .unwrap();
            p.bob.add_contact(p.alice_at_bob.clone()).unwrap(); // no ratchet
            (&p.alice, "alice@old.com", &p.bob, bob_id, alice_id)
        } else {
            p.bob
                .save_exchanged_contact(&p.alice_at_bob, &p.bob_ratchet, true)
                .unwrap();
            p.alice.add_contact(p.bob_at_alice.clone()).unwrap(); // no ratchet
            (&p.bob, "bob@old.com", &p.alice, alice_id, bob_id)
        };

    // Sender (initiator) edits its email and prepares the encrypted update.
    let old_card = sender.own_card().unwrap().unwrap();
    let field_id = old_card.fields().first().unwrap().id().to_string();
    let mut new_card = old_card.clone();
    new_card
        .update_field_value(&field_id, "updated@new.com", 1)
        .unwrap();
    sender.update_own_card(&new_card).unwrap();
    let new_card = sender.own_card().unwrap().unwrap();
    let encrypted = common::card_update::seal_update_default(
        sender,
        &recipient_id_at_sender,
        &old_card,
        &new_card,
    );

    // The ratchet-less recipient cannot decrypt → NoRatchetState.
    let result = process_single_card_update(
        recipient.identity().unwrap(),
        recipient.storage(),
        &sender_id_at_recipient,
        &encrypted,
    );
    assert!(
        matches!(result, Err(CardUpdateError::NoRatchetState)),
        "ratchet-less contact must reject the update with NoRatchetState, got {result:?}"
    );

    // And the recipient's stored card is unchanged — the edit never crossed.
    let stored = recipient
        .get_contact(&sender_id_at_recipient)
        .unwrap()
        .unwrap();
    assert!(
        stored
            .card()
            .fields()
            .iter()
            .all(|f| f.value() != "updated@new.com"),
        "the update must NOT apply at a ratchet-less peer"
    );
    assert!(
        stored
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == sender_old_value),
        "the recipient must still hold the sender's pre-update value"
    );
}

/// Coverage for the device scenario (2026-06-28-sync-delivery-sent-not-received):
/// both parties onboard with an EMPTY card (skipped contact info), exchange,
/// then the initiator ADDS its first field and syncs. The existing round-trips
/// above exchange a card that ALREADY has a field and UPDATE its value; this
/// pins the empty-exchanged-card + first-Add-delta + first-CEK path the two
/// phones actually hit. It **passes** — so that path is NOT the device cause
/// (the on-device `rejected=1` lives in a layer these in-core tests don't
/// exercise: the live exchange's ratchet state, the `EncryptedUpdate` wire
/// round-trip, or the at-rest ratchet-blob crypto).
// @scenario: sync_updates :: First field added to an empty exchanged card propagates
#[test]
fn multistage_first_field_add_to_empty_card_round_trips() {
    let alice = create_vauchi_with_identity("Alice");
    let bob = create_vauchi_with_identity("Bob");

    let alice_pk = *alice.identity().unwrap().signing_public_key();
    let bob_pk = *bob.identity().unwrap().signing_public_key();
    let alice_card = alice.own_card().unwrap().unwrap();
    let bob_card = bob.own_card().unwrap().unwrap();

    let (alice_session, bob_session) = drive_to_finalized(b"alice".to_vec(), b"bob".to_vec());
    let alice_tk = alice_session.get_transport_key().expect("alice tk");
    let bob_tk = bob_session.get_transport_key().expect("bob tk");
    assert_eq!(alice_tk, bob_tk, "transport key must be symmetric");

    let (alice_ratchet, alice_is_initiator) = alice_session
        .build_exchange_ratchet(&alice_pk, &bob_pk)
        .expect("alice ratchet builds");
    let (bob_ratchet, bob_is_initiator) = bob_session
        .build_exchange_ratchet(&bob_pk, &alice_pk)
        .expect("bob ratchet builds");
    assert_ne!(alice_is_initiator, bob_is_initiator);

    let bob_at_alice =
        Contact::from_exchange(bob_pk, bob_card, SymmetricKey::from_bytes(alice_tk), 0);
    let alice_at_bob =
        Contact::from_exchange(alice_pk, alice_card, SymmetricKey::from_bytes(bob_tk), 0);
    let bob_id_at_alice = bob_at_alice.id().to_string();
    let alice_id_at_bob = alice_at_bob.id().to_string();

    alice
        .save_exchanged_contact(&bob_at_alice, &alice_ratchet, alice_is_initiator)
        .unwrap();
    bob.save_exchanged_contact(&alice_at_bob, &bob_ratchet, bob_is_initiator)
        .unwrap();

    // The initiator speaks first (responder has no sending chain pre-receive),
    // matching the device: Bob (initiator) added a field and synced.
    let (sender, recipient, recipient_id_at_sender, sender_id_at_recipient) = if alice_is_initiator
    {
        (
            &alice,
            &bob,
            bob_id_at_alice.clone(),
            alice_id_at_bob.clone(),
        )
    } else {
        (
            &bob,
            &alice,
            alice_id_at_bob.clone(),
            bob_id_at_alice.clone(),
        )
    };

    // The sender's FIRST field added to its previously-empty card.
    let old_card = sender.own_card().unwrap().unwrap();
    assert!(
        old_card.fields().is_empty(),
        "precondition: the exchanged card was empty"
    );
    sender
        .add_own_field(ContactField::new(
            FieldType::Email,
            "personal",
            "fresh@new.com",
            1,
        ))
        .unwrap();
    let new_card = sender.own_card().unwrap().unwrap();

    let encrypted = common::card_update::seal_update_default(
        sender,
        &recipient_id_at_sender,
        &old_card,
        &new_card,
    );

    process_single_card_update(
        recipient.identity().unwrap(),
        recipient.storage(),
        &sender_id_at_recipient,
        &encrypted,
    )
    .expect("responder must decrypt the initiator's FIRST add to an empty card");

    let stored = recipient
        .get_contact(&sender_id_at_recipient)
        .unwrap()
        .unwrap();
    assert!(
        stored
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "fresh@new.com"),
        "the added field must reflect at the peer after the first decrypt"
    );
}

// 2026-06-30 device root-cause (Pixel 3a <-> S7): the mobile exchange persisted
// the role-correct ratchet but never armed the initiator's first send, so NO
// card update ever flowed — a responder that edited first hit
// "no sending chain (responder must receive first)" and its error was swallowed,
// and if neither side edited nothing propagated at all. The existing round-trip
// tests masked this by always doing an explicit initiator-first edit (their
// `assert_card_update_round_trips` IS the bootstrap). The fix arms repropagation
// inside `save_exchanged_contact`, so the exchange itself bootstraps on the next
// sync with NO explicit edit. This test asserts that, then that the responder's
// own owed send succeeds once the initiator's bootstrap has been received.
// @scenario: sync_updates :: A multi-stage exchange auto-bootstraps card sync without an edit
#[test]
fn multistage_exchange_arms_bootstrap_without_an_explicit_edit() {
    let p = multistage_exchanged_pair();
    let bob_id = p.bob_at_alice.id().to_string();
    let alice_id = p.alice_at_bob.id().to_string();

    // `create_vauchi_with_card` armed the repropagate marker via `add_own_field`;
    // clear it so the EXCHANGE is the only thing that can arm a bootstrap — this
    // mirrors the device, where onboarding added no field before the exchange.
    let clear = vauchi_core::types::OwnCardRepropagateState::default();
    p.alice
        .storage()
        .ux()
        .save_own_card_repropagate(&clear)
        .unwrap();
    p.bob
        .storage()
        .ux()
        .save_own_card_repropagate(&clear)
        .unwrap();

    p.alice
        .save_exchanged_contact(&p.bob_at_alice, &p.alice_ratchet, p.alice_is_initiator)
        .unwrap();
    p.bob
        .save_exchanged_contact(&p.alice_at_bob, &p.bob_ratchet, p.bob_is_initiator)
        .unwrap();

    // (initiator, responder, initiator's-target-id, responder's-target-id)
    let (initiator, responder, init_target, resp_target) = if p.alice_is_initiator {
        (&p.alice, &p.bob, &bob_id, &alice_id)
    } else {
        (&p.bob, &p.alice, &alice_id, &bob_id)
    };

    // No explicit edit: the exchange alone must have armed repropagation, so the
    // initiator's owed pass queues a bootstrap send to the new contact.
    initiator.run_owed_repropagation().unwrap();
    let bootstrap = initiator
        .storage()
        .pending()
        .get_pending_updates(init_target)
        .unwrap();
    assert_eq!(
        bootstrap.len(),
        1,
        "the exchange must arm an initiator bootstrap send with no explicit edit"
    );

    // The responder's owed pass, armed by the same exchange, cannot send yet —
    // it has no sending chain until it receives. The error is swallowed, nothing
    // is queued.
    responder.run_owed_repropagation().unwrap();
    assert_eq!(
        responder
            .storage()
            .pending()
            .get_pending_updates(resp_target)
            .unwrap()
            .len(),
        0,
        "a responder cannot send before receiving the initiator's first message"
    );

    // Deliver the bootstrap: the responder receives → its sending chain is
    // established. Its next owed pass now succeeds where it previously errored.
    for upd in &bootstrap {
        process_single_card_update(
            responder.identity().unwrap(),
            responder.storage(),
            resp_target,
            &upd.payload,
        )
        .unwrap();
    }
    responder.run_owed_repropagation().unwrap();
    assert_eq!(
        responder
            .storage()
            .pending()
            .get_pending_updates(resp_target)
            .unwrap()
            .len(),
        1,
        "after receiving the initiator's bootstrap, the responder can finally send"
    );
}
