// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The hand-back follows the meeting's exchange by itself (vauchi/private#796
//! step 2c, D14): an exchange with the new key of a claim this guardian
//! vouched for queues it, and a guardian that cannot send yet (the
//! exchange's responder) owes it until the next sync after the new phone's
//! first message. It is sent once, and an ordinary exchange sends nothing.

use vauchi_core::Vauchi;
use vauchi_core::api::ReceiveOutcome;

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{deliver, meet, signing_pk};
use crate::common::two_recipient::{Recipient, add_recipient, receive};

fn returned_pieces(outcomes: &[ReceiveOutcome]) -> usize {
    outcomes
        .iter()
        .filter(|outcome| matches!(outcome, ReceiveOutcome::ReturnedBackupPiece(_)))
        .count()
}

/// Alice's three guardians keep her backup; the first vouches for her new
/// phone's claim.
fn vouched_meeting() -> (Vec<Recipient>, Vauchi) {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let mut guardians: Vec<Recipient> = (0..3)
        .map(|index| add_recipient(&alice, &alice_pk, &format!("G{index}")))
        .collect();
    for guardian in &guardians {
        alice
            .verify_contact_fingerprint(&guardian.id_at_sharer)
            .unwrap();
        alice.toggle_recovery_trust(&guardian.id_at_sharer).unwrap();
    }
    for guardian in &mut guardians {
        receive(&alice, guardian);
    }
    let phone = create_vauchi_with_identity("Alice, new phone");
    let claim = phone
        .create_recovery_claim_hex_b64(&hex::encode(alice_pk))
        .unwrap();
    guardians[0]
        .vauchi()
        .create_voucher_from_claim_b64(&claim)
        .unwrap();
    (guardians, phone)
}

// @internal
#[test]
fn an_initiating_guardian_hands_back_as_the_exchange_completes() {
    let (guardians, phone) = vouched_meeting();
    let g = guardians[0].vauchi();

    let (phone_at_g, g_at_phone) = meet(g, &phone, true);

    assert!(returned_pieces(&deliver(g, &phone_at_g, &phone, &g_at_phone)) > 0);
    assert!(
        phone
            .storage()
            .returned_backups()
            .kept(&g_at_phone)
            .unwrap()
            .is_some_and(|returned| returned.sealed_share().len() == 124)
    );
}

// @internal
#[test]
fn a_responding_guardian_owes_the_hand_back_until_the_phone_has_written_and_sends_it_once() {
    let (guardians, phone) = vouched_meeting();
    let g = guardians[0].vauchi();

    let (phone_at_g, g_at_phone) = meet(g, &phone, false);
    assert_eq!(g.hand_back_owed().unwrap(), 0, "no sending chain yet");
    assert_eq!(
        g.storage()
            .pending()
            .get_pending_updates(&phone_at_g)
            .unwrap()
            .len(),
        0
    );

    phone.request_card_resync().unwrap();
    deliver(&phone, &g_at_phone, g, &phone_at_g);

    assert_eq!(g.hand_back_owed().unwrap(), 1);
    assert!(returned_pieces(&deliver(g, &phone_at_g, &phone, &g_at_phone)) > 0);
    assert!(
        phone
            .storage()
            .returned_backups()
            .kept(&g_at_phone)
            .unwrap()
            .is_some()
    );
    assert_eq!(g.hand_back_owed().unwrap(), 0, "sent once");
}

// @internal
#[test]
fn an_ordinary_exchange_hands_nothing_back() {
    let (guardians, _phone) = vouched_meeting();
    let g = guardians[0].vauchi();
    let stranger = create_vauchi_with_identity("Stranger");

    let (stranger_at_g, g_at_stranger) = meet(g, &stranger, true);

    assert_eq!(
        returned_pieces(&deliver(g, &stranger_at_g, &stranger, &g_at_stranger)),
        0
    );
    assert_eq!(g.hand_back_owed().unwrap(), 0);
}
