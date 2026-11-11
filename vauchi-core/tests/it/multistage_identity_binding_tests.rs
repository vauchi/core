// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A multi-stage exchange binds the identity key its peer claims
//! (vauchi/private#1217; problem record
//! 2026-11-11-multistage-identity-binding, Tamarin
//! `verification/multistage_exchange.spthy`).
//!
//! The payload carries the identity key and a signature by it over the
//! sender's session id, ephemeral, relay URL and card. A session that cannot
//! verify it never finalizes, so the app never sees the card. Before the fix
//! Mallory, holding only Bob's public key, replaced the card Alice keeps for
//! Bob (reproduced on core 03c434e74).

use vauchi_core::exchange::multistage::qr_codec;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::{ContactCard, ContactField, FieldType, Identity};

fn card(name: &str, email: &str) -> ContactCard {
    let mut card = ContactCard::new(name);
    card.add_field(ContactField::new(FieldType::Email, "email", email, 0))
        .unwrap();
    card
}

fn exchange(a: &mut MultiStageSession, b: &mut MultiStageSession) {
    let a_init = a.get_display_qr().unwrap();
    let b_init = b.get_display_qr().unwrap();
    a.process_scanned_qr(&b_init.data);
    b.process_scanned_qr(&a_init.data);
    for _ in 0..200 {
        let aq = a.get_display_qr();
        let bq = b.get_display_qr();
        if let Some(q) = &aq {
            b.process_scanned_qr(&q.data);
        }
        if let Some(q) = &bq {
            a.process_scanned_qr(&q.data);
        }
        let done = |s: &MultiStageSession| {
            matches!(
                s.get_state(),
                ProtocolState::Finalized | ProtocolState::Failed(_)
            )
        };
        if done(a) && done(b) {
            break;
        }
    }
}

/// Mallory's payload in the v2 layout, `[2][identity][signature][card]`,
/// claiming `claimed` with a signature she could not have made.
fn forged_payload(claimed: &[u8; 32], card: &ContactCard) -> Vec<u8> {
    let mut out = vec![2u8];
    out.extend_from_slice(claimed);
    out.extend_from_slice(&[0u8; 64]);
    out.extend_from_slice(&serde_json::to_vec(card).unwrap());
    out
}

// @internal
#[test]
fn two_honest_phones_finalize_and_each_reads_the_others_identity_and_card() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    let mut b = MultiStageSession::for_identity(&bob, &card("Bob", "b@b.example"), None);

    exchange(&mut a, &mut b);

    assert_eq!(a.get_state(), ProtocolState::Finalized);
    assert_eq!(b.get_state(), ProtocolState::Finalized);
    let bob_at_alice = a
        .received_peer()
        .expect("Alice holds Bob's verified payload");
    assert_eq!(&bob_at_alice.identity_pk, bob.signing_public_key());
    assert_eq!(bob_at_alice.card.fields()[0].value(), "b@b.example");
    let alice_at_bob = b
        .received_peer()
        .expect("Bob holds Alice's verified payload");
    assert_eq!(&alice_at_bob.identity_pk, alice.signing_public_key());
}

// @internal
#[test]
fn a_peer_claiming_an_identity_it_cannot_sign_for_never_finalizes() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    let mut mallory = MultiStageSession::new(forged_payload(
        bob.signing_public_key(),
        &card("Bob", "bob@attacker.example"),
    ));

    exchange(&mut a, &mut mallory);

    assert!(
        matches!(a.get_state(), ProtocolState::Failed(_)),
        "Alice accepted a payload claiming Bob's key without Bob's signature: {:?}",
        a.get_state()
    );
    assert!(a.received_peer().is_none());
}

// @internal
#[test]
fn an_unsigned_v1_payload_is_refused() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);
    let mut v1 = vec![1u8];
    v1.extend_from_slice(bob.signing_public_key());
    v1.extend_from_slice(&serde_json::to_vec(&card("Bob", "b@b.example")).unwrap());
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    let mut old = MultiStageSession::new(v1);

    exchange(&mut a, &mut old);

    assert!(
        matches!(a.get_state(), ProtocolState::Failed(_)),
        "{:?}",
        a.get_state()
    );
    assert!(a.received_peer().is_none());
}

// @internal
#[test]
fn a_peer_carrying_our_own_identity_is_refused() {
    // Two devices of one identity: the signature is valid, the identity is ours.
    let alice = Identity::create("Alice", 0);
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    let mut twin = MultiStageSession::for_identity(&alice, &card("Alice", "a2@a.example"), None);

    exchange(&mut a, &mut twin);

    // Whichever refuses first stops showing frames, so the other may be
    // left waiting rather than failed; neither finalizes.
    for session in [&a, &twin] {
        assert_ne!(session.get_state(), ProtocolState::Finalized);
        assert!(session.received_peer().is_none());
    }
    assert!(
        [&a, &twin]
            .iter()
            .any(|s| matches!(s.get_state(), ProtocolState::Failed(_))),
        "one of them refused the other: {:?} / {:?}",
        a.get_state(),
        twin.get_state()
    );
}

// @internal
#[test]
fn an_init_carrying_the_ciphertext_in_clear_moves_nothing() {
    let alice = Identity::create("Alice", 0);
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    let before = a.get_state();
    let init = qr_codec::format_ini2_qr_with_relay(&[7; 16], &[9; 32], &[3; 32], "Bob", None);
    let in3d = format!("IN3D{}", &init[4..]);

    a.process_scanned_qr(&in3d);

    assert_eq!(
        a.get_state(),
        before,
        "IN3D bypasses transport encryption and is refused"
    );
}

// @internal
#[test]
fn bobs_frames_recorded_in_an_exchange_with_mallory_and_replayed_to_alice_never_finalize() {
    // Mallory exchanged with Bob honestly and recorded every frame he showed;
    // shown to Alice, they carry Bob's signed payload, but Mallory holds
    // neither Bob's ephemeral secret nor a key to confirm with.
    let bob = Identity::create("Bob", 0);
    let mallory = Identity::create("Mallory", 0);
    let mut b = MultiStageSession::for_identity(&bob, &card("Bob", "b@b.example"), None);
    let mut m = MultiStageSession::for_identity(&mallory, &card("M", "m@m.example"), None);
    let mut recorded = vec![b.get_display_qr().unwrap().data];
    m.process_scanned_qr(&recorded[0]);
    b.process_scanned_qr(&m.get_display_qr().unwrap().data);
    for _ in 0..200 {
        if let Some(q) = b.get_display_qr() {
            m.process_scanned_qr(&q.data);
            recorded.push(q.data);
        }
        if let Some(q) = m.get_display_qr() {
            b.process_scanned_qr(&q.data);
        }
    }
    assert_eq!(
        b.get_state(),
        ProtocolState::Finalized,
        "the recording is complete"
    );
    let alice = Identity::create("Alice", 0);
    let mut a = MultiStageSession::for_identity(&alice, &card("Alice", "a@a.example"), None);
    a.get_display_qr();

    for _ in 0..3 {
        for frame in &recorded {
            a.process_scanned_qr(frame);
            a.get_display_qr();
        }
    }

    assert_ne!(a.get_state(), ProtocolState::Finalized);
    assert!(a.received_peer().is_none());
}
