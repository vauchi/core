// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The hand-back follows the meeting's exchange by itself (vauchi/private#796
//! step 2c, D14): an exchange with the new key of a claim this guardian
//! vouched for queues it, and a guardian that cannot send yet (the
//! exchange's responder) owes it until the next sync after the new phone's
//! first message. It is sent once, and an ordinary exchange sends nothing.

use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::{SymmetricKey, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::two_recipient::{Recipient, add_recipient, receive};

fn signing_pk(wb: &Vauchi) -> [u8; 32] {
    *wb.identity().unwrap().signing_public_key()
}

/// An in-person exchange through the production seam,
/// `save_exchanged_contact`, with `guardian_initiates` choosing the roles.
/// Returns the phone's id at the guardian and the guardian's at the phone.
fn meet(guardian: &Vauchi, phone: &Vauchi, guardian_initiates: bool) -> (String, String) {
    let shared = SymmetricKey::generate();
    let phone_at_g = Contact::from_exchange(
        signing_pk(phone),
        ContactCard::new("Phone"),
        shared.clone(),
        0,
    );
    let g_at_phone = Contact::from_exchange(
        signing_pk(guardian),
        ContactCard::new("G"),
        shared.clone(),
        0,
    );
    let responder_dh = X3DHKeyPair::generate();
    let initiator =
        DoubleRatchetState::initialize_initiator(&shared, *responder_dh.public_key()).unwrap();
    let responder = DoubleRatchetState::initialize_responder(&shared, responder_dh);
    let (g_ratchet, phone_ratchet) = if guardian_initiates {
        (initiator, responder)
    } else {
        (responder, initiator)
    };
    phone
        .save_exchanged_contact(&g_at_phone, &phone_ratchet, !guardian_initiates)
        .unwrap();
    guardian
        .save_exchanged_contact(&phone_at_g, &g_ratchet, guardian_initiates)
        .unwrap();
    (phone_at_g.id().to_string(), g_at_phone.id().to_string())
}

/// Delivers what `from` queued for `to_id` into `to` (from `from_id`) and
/// returns what `to` made of it.
fn deliver(from: &Vauchi, to_id: &str, to: &Vauchi, from_id: &str) -> Vec<ReceiveOutcome> {
    let pending = from.storage().pending().get_pending_updates(to_id).unwrap();
    for update in &pending {
        from.storage()
            .pending()
            .mark_update_sent(&update.id)
            .unwrap();
    }
    pending
        .iter()
        .map(|update| {
            process_single_card_update(
                to.identity().unwrap(),
                to.storage(),
                from_id,
                &update.payload,
            )
            .unwrap()
        })
        .collect()
}

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
