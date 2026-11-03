// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A thief holding Alice's old device unlocked holds the old seed, every
//! shared key and every guardian token, and signs anything the old key
//! signs. None of that may move one of her contacts to another identity
//! (security review 2026-11-03, hard requirement; vauchi/private#796): a
//! rekey needs vouchers the contact can place on keys the thief does not
//! hold, and an old-key signature authorizes no merge.

use std::sync::Arc;
use std::time::SystemTime;

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::api::sync::process_single_card_update;
use vauchi_core::clock::FakeClock;
use vauchi_core::recovery::rekey::RecoveryContinuation;
use vauchi_core::sync::delta::encode_recovery_continuation;
use vauchi_core::{SigningKeyPair, Vauchi};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::{meet, signing_pk};
use crate::common::recovered_phone::{World, dave_forgets_the_guardians, recovered};

/// The thief's phone, restored from the backup its sock-puppet guardians
/// handed back, with the offer it makes Dave, who knows none of them.
fn thiefs_offer() -> (World, Vec<u8>) {
    let clock = Arc::new(FakeClock::new(SystemTime::now()));
    let mut phone = Vauchi::in_memory_with_clock(clock).unwrap();
    phone.create_identity("Alice").unwrap();
    let world = recovered(phone, None, 3);
    dave_forgets_the_guardians(&world);
    world.phone.restore_from_returned_backups(None).unwrap();
    let blob = world.phone.rekey_offer_deliveries().unwrap()[0]
        .blob
        .clone();
    (world, blob)
}

/// Seals `payload` from `sender` to `recipient_id_at_sender` on their
/// ratchet, as the thief's phone would send anything it likes.
fn sealed(sender: &Vauchi, recipient_id_at_sender: &str, payload: &[u8]) -> Vec<u8> {
    let ratchets = sender.storage().ratchets();
    let (mut ratchet, is_initiator) = ratchets
        .load_ratchet_state(recipient_id_at_sender)
        .unwrap()
        .unwrap();
    let message = ratchet.encrypt(payload).unwrap();
    ratchets
        .save_ratchet_state(recipient_id_at_sender, &ratchet, is_initiator)
        .unwrap();
    serde_json::to_vec(&message).unwrap()
}

// @internal
#[test]
fn an_offer_vouched_only_by_guardians_the_contact_cannot_place_is_dropped() {
    let (world, blob) = thiefs_offer();
    let dave = world.dave.vauchi();
    let alice_at_dave = world.dave.sharer_id().to_string();

    let taken = dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
    assert!(dave.get_contact(&alice_at_dave).unwrap().is_some());
    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
}

// @internal
#[test]
fn meeting_the_thief_in_person_does_not_let_that_offer_fold_alice_into_it() {
    let (world, blob) = thiefs_offer();
    let dave = world.dave.vauchi();
    let alice_at_dave = world.dave.sharer_id().to_string();
    let (_, thief_at_dave) = meet(&world.phone, dave, true);

    let taken = dave.take_rekey_offer(&blob).unwrap();

    assert_eq!(taken, RekeyOfferTaken::Dropped);
    assert!(dave.get_contact(&alice_at_dave).unwrap().is_some());
    assert!(dave.get_contact(&thief_at_dave).unwrap().is_some());
}

// @internal
#[test]
fn a_continuation_signed_with_the_old_key_merges_nothing() {
    let (world, _) = thiefs_offer();
    let dave = world.dave.vauchi();
    let alice_at_dave = world.dave.sharer_id().to_string();
    let thief = create_vauchi_with_identity("Alice");
    let (dave_at_thief, thief_at_dave) = meet(&thief, dave, true);
    let old_keys = SigningKeyPair::from_seed(world.alice.identity().unwrap().master_seed());
    let continuation = RecoveryContinuation::create(&old_keys, &signing_pk(&thief));
    let message = sealed(
        &thief,
        &dave_at_thief,
        &encode_recovery_continuation(&continuation),
    );

    let _received = process_single_card_update(
        dave.identity().unwrap(),
        dave.storage(),
        &thief_at_dave,
        &message,
    );

    assert!(
        dave.get_contact(&alice_at_dave).unwrap().is_some(),
        "Alice stays Alice"
    );
    assert!(dave.get_contact(&thief_at_dave).unwrap().is_some());
}
