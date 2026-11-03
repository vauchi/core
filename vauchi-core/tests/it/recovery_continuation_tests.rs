// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact Alice's new phone met in person (vauchi/private#796). The
//! continuation D20 sent, signed with her old key, merges nothing any
//! more: an old-key signature authorizes no merge (security review
//! 2026-11-03), so the phone sends none. A contact met again is offered the
//! rekey like any other, and folds its old contact into the meeting's only
//! when the proof shows the recovery on vouchers of its own contacts. The
//! continuation's wire form stays, so a receiver decodes it and moves on.

use vauchi_core::api::RekeyOfferTaken;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::recovery::rekey::RecoveryContinuation;
use vauchi_core::{Storage, SymmetricKey};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::meet;
use crate::common::recovered_phone::recovered;

// @internal
#[test]
fn a_contact_met_after_the_restore_is_offered_the_rekey_and_folds_on_its_proof() {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);
    world.phone.restore_from_returned_backups(None).unwrap();
    let dave = world.dave.vauchi();
    let alice_old_at_dave = world.dave.sharer_id().to_string();
    let (phone_at_dave, _) = meet(dave, &world.phone, false);
    let met = dave.get_contact(&phone_at_dave).unwrap().unwrap();
    let offer = world
        .phone
        .rekey_offer_deliveries()
        .unwrap()
        .into_iter()
        .find(|delivery| delivery.contact_id == world.dave_id)
        .expect("a contact met again is offered the rekey too")
        .blob;

    let taken = world.dave_takes_as_authorized(&offer);

    assert_eq!(
        taken,
        RekeyOfferTaken::Accepted {
            contact_id: phone_at_dave.clone()
        }
    );
    assert!(dave.get_contact(&alice_old_at_dave).unwrap().is_none());
    let folded = dave.get_contact(&phone_at_dave).unwrap().unwrap();
    assert_eq!(
        folded.shared_key().unwrap().as_bytes(),
        met.shared_key().unwrap().as_bytes(),
        "the meeting's relationship stays"
    );
}

// @internal
#[test]
fn a_guardian_met_before_the_restore_is_offered_the_rekey_too() {
    let world = recovered(create_vauchi_with_identity("Alice, new phone"), None, 3);

    world.phone.restore_from_returned_backups(None).unwrap();

    let offered: Vec<String> = world
        .phone
        .rekey_offer_deliveries()
        .unwrap()
        .into_iter()
        .map(|delivery| delivery.contact_id)
        .collect();
    for guardian in &world.guardians_at_phone {
        assert!(offered.contains(guardian), "{guardian}");
    }
}

// @internal
#[test]
fn a_continuation_round_trips_and_a_flipped_byte_breaks_it() {
    let old_keys = SigningKeyPair::generate();
    let new_pk = [5; 32];
    let continuation = RecoveryContinuation::create(&old_keys, &new_pk);

    let bytes = continuation.encode();
    assert_eq!(RecoveryContinuation::decode(&bytes).unwrap(), continuation);
    assert!(continuation.verify());
    for at in 0..bytes.len() {
        let mut flipped = bytes.clone();
        flipped[at] ^= 1;
        assert!(
            RecoveryContinuation::decode(&flipped).is_ok_and(|c| !c.verify()),
            "{at}"
        );
    }
    assert!(RecoveryContinuation::decode(&bytes[1..]).is_err());
}

// @internal
#[test]
fn the_old_identitys_seed_kept_at_restore_survives_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage.recovery().save_old_identity_seed(&[3; 32]).unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage
            .recovery()
            .load_old_identity_seed()
            .unwrap()
            .as_deref(),
        Some(&[3u8; 32])
    );
}
