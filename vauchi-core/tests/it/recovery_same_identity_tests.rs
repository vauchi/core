// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Social recovery restores the same identity (vauchi/private#796, P28,
//! P32). The new phone starts with no identity, holding a temporary
//! recovery key its claim names beside the old identity's key; each
//! guardian, in person, hands back its share re-sealed to that key with
//! the backup; with enough of them the phone becomes the old identity as
//! a new device of it. Contacts come back with their shared keys and
//! nothing waits for a rekey.

use vauchi_core::sync::guardian_piece::GuardianDelivery;
use vauchi_core::{AuthMode, Storage, SymmetricKey, Vauchi, VauchiConfig};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::meeting::signing_pk;
use crate::common::two_recipient::{Recipient, add_recipient, add_recipient_from, receive};

/// Alice with Dave as a contact and three guardians, each in a group of
/// its own, holding her backup.
struct Lost {
    alice: Vauchi,
    dave: Recipient,
    guardians: Vec<Recipient>,
}

fn lost() -> Lost {
    let alice = create_vauchi_with_identity("Alice");
    let alice_pk = signing_pk(&alice);
    let dave = add_recipient_from(&alice, &alice_pk, create_vauchi_with_identity("Dave"));
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
    Lost {
        alice,
        dave,
        guardians,
    }
}

fn alice_pk_hex(lost: &Lost) -> String {
    hex::encode(signing_pk(&lost.alice))
}

/// The new phone begins a recovery and takes the hand-backs of the first
/// `handing_back` guardians.
fn handed_back(lost: &Lost, handing_back: usize) -> Vauchi {
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(lost)).unwrap();
    for guardian in lost.guardians.iter().take(handing_back) {
        let hand_back = guardian.vauchi().hand_back_in_person(&claim).unwrap();
        phone.take_hand_back(&hand_back).unwrap();
    }
    phone
}

// @internal
#[test]
fn a_phone_without_an_identity_begins_a_recovery_naming_the_old_key_and_a_temporary_one() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();

    let claim_b64 = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();

    let claim = lost.guardians[0]
        .vauchi()
        .parse_recovery_claim_b64(&claim_b64)
        .unwrap();
    assert_eq!(claim.old_pk().as_bytes(), &signing_pk(&lost.alice));
    assert_ne!(claim.new_pk().as_bytes(), &signing_pk(&lost.alice));
    assert!(!phone.has_identity(), "no identity until the restore");
}

// @internal
#[test]
fn beginning_again_keeps_the_same_temporary_key() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let guardian = lost.guardians[0].vauchi();

    let first = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let again = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();

    assert_eq!(
        guardian.parse_recovery_claim_b64(&first).unwrap().new_pk(),
        guardian.parse_recovery_claim_b64(&again).unwrap().new_pk()
    );
}

// @internal
#[test]
fn a_phone_with_an_identity_cannot_begin_a_recovery() {
    let lost = lost();
    let phone = create_vauchi_with_identity("Someone");

    assert!(phone.begin_recovery(&alice_pk_hex(&lost)).is_err());
}

// @internal
#[test]
fn a_guardian_hands_back_only_for_an_identity_whose_backup_it_keeps() {
    let lost = lost();
    let stranger = create_vauchi_with_identity("Stranger");
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone
        .begin_recovery(&hex::encode(signing_pk(&stranger)))
        .unwrap();

    assert!(
        lost.guardians[0]
            .vauchi()
            .hand_back_in_person(&claim)
            .is_err()
    );
}

// @internal
#[test]
fn the_hand_backs_restore_the_same_identity_on_a_new_device() {
    let lost = lost();
    let mut phone = handed_back(&lost, 3);

    let restored = phone.restore_identity_from_hand_backs(None).unwrap();

    assert!(restored >= 1);
    let identity = phone.identity().unwrap();
    let alice = lost.alice.identity().unwrap();
    assert_eq!(identity.signing_public_key(), alice.signing_public_key());
    assert_ne!(
        identity.device_id(),
        alice.device_id(),
        "a new device of the identity, so the lost one can be revoked"
    );
    let dave_at_phone = phone.get_contact(&lost.dave.id_at_sharer).unwrap().unwrap();
    let dave_at_alice = lost
        .alice
        .get_contact(&lost.dave.id_at_sharer)
        .unwrap()
        .unwrap();
    assert_eq!(
        dave_at_phone.shared_key().unwrap().as_bytes(),
        dave_at_alice.shared_key().unwrap().as_bytes()
    );
    assert!(
        phone.rekey_pending_contacts().unwrap().is_empty(),
        "nothing waits for a rekey"
    );
}

// @internal
#[test]
fn too_few_hand_backs_restore_nothing() {
    let lost = lost();
    let mut phone = handed_back(&lost, 2);

    assert!(phone.restore_identity_from_hand_backs(None).is_err());
    assert!(!phone.has_identity());
}

// @internal
#[test]
fn taking_the_same_hand_back_twice_keeps_it_once() {
    let lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let hand_back = lost.guardians[0]
        .vauchi()
        .hand_back_in_person(&claim)
        .unwrap();

    assert_eq!(phone.take_hand_back(&hand_back).unwrap(), 1);
    assert_eq!(phone.take_hand_back(&hand_back).unwrap(), 1);
}

/// A coerced guardian answers as one that holds no backup (ADR-032).
// @internal
#[test]
fn a_duress_session_hands_back_nothing_and_answers_as_one_with_no_backup() {
    let mut lost = lost();
    let phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let stranger_claim = Vauchi::in_memory()
        .unwrap()
        .begin_recovery(&hex::encode([9u8; 32]))
        .unwrap();
    let mut guardian = lost.guardians.remove(0).into_vauchi();
    let no_backup = format!("{:?}", guardian.hand_back_in_person(&stranger_claim));
    guardian.setup_app_password("normal-pin").unwrap();
    guardian.setup_duress_password("112233").unwrap();
    assert_eq!(guardian.authenticate("112233").unwrap(), AuthMode::Duress);

    let coerced = guardian.hand_back_in_person(&claim);

    assert!(coerced.is_err());
    assert_eq!(format!("{coerced:?}"), no_backup);
}

// @internal
#[test]
fn the_recovery_key_and_hand_backs_survive_a_storage_rekey() {
    let mut storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    storage
        .recovery()
        .save_recovery_mode_seed(&[4; 32])
        .unwrap();
    storage
        .recovery()
        .keep_hand_back(&[5; 32], b"hand-back", 7)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage
            .recovery()
            .load_recovery_mode_seed()
            .unwrap()
            .as_deref(),
        Some(&[4u8; 32])
    );
    assert_eq!(
        storage.recovery().hand_backs().unwrap(),
        vec![b"hand-back".to_vec()]
    );
}

/// Guardians of another identity seal that identity's backup to the
/// recovery key the claim shows; the restore must not adopt it.
// @internal
#[test]
fn a_backup_of_another_identity_restores_nothing() {
    let wanted = lost();
    let other = lost();
    let mut phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&wanted)).unwrap();
    let recovery_pk = *other.guardians[0]
        .vauchi()
        .parse_recovery_claim_b64(&claim)
        .unwrap()
        .new_pk()
        .as_bytes();
    for guardian in &other.guardians {
        let g = guardian.vauchi();
        let kept = g
            .storage()
            .guardian_backups()
            .kept(&alice_pk_hex(&other))
            .unwrap()
            .unwrap();
        let resealed = g
            .respond_to_recovery(kept.sealed_share(), &recovery_pk)
            .unwrap();
        let forged = GuardianDelivery::new(kept.created_at(), resealed, kept.backup().to_vec())
            .unwrap()
            .encode();
        phone.take_hand_back(&forged).unwrap();
    }

    assert!(phone.restore_identity_from_hand_backs(None).is_err());
    assert!(!phone.has_identity());
}

/// The restored phone registers itself and lists the lost device as
/// revoked, and both survive a restart.
// @internal
#[test]
fn the_restored_phone_registers_itself_and_the_lost_device_as_revoked() {
    let lost = lost();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("phone.db");
    let key = SymmetricKey::generate();
    {
        let mut phone =
            Vauchi::new(VauchiConfig::with_storage_path(&path).with_storage_key(key.clone()))
                .unwrap();
        let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
        for guardian in &lost.guardians {
            let hand_back = guardian.vauchi().hand_back_in_person(&claim).unwrap();
            phone.take_hand_back(&hand_back).unwrap();
        }
        phone.restore_identity_from_hand_backs(None).unwrap();
    }

    let phone = Vauchi::new(VauchiConfig::with_storage_path(&path).with_storage_key(key)).unwrap();

    let devices = phone.list_devices().unwrap();
    assert_eq!(devices.len(), 2, "{devices:?}");
    let current: Vec<_> = devices.iter().filter(|d| d.is_current).collect();
    assert_eq!(current.len(), 1);
    assert!(current[0].is_active);
    let lost_device = devices.iter().find(|d| !d.is_current).unwrap();
    assert!(!lost_device.is_active, "the lost device is revoked");
    let registry = phone
        .storage()
        .device()
        .load_device_registry()
        .unwrap()
        .unwrap();
    assert!(registry.verify(&phone.identity().unwrap().signing_keypair().public_key()));
}

/// A guardian whose hand-back is retried many times counts once, and the
/// retries do not stop the restore.
// @internal
#[test]
fn retried_hand_backs_count_once_and_do_not_stop_the_restore() {
    let lost = lost();
    let mut phone = Vauchi::in_memory().unwrap();
    let claim = phone.begin_recovery(&alice_pk_hex(&lost)).unwrap();
    let first = lost.guardians[0].vauchi();
    for _ in 0..25 {
        let hand_back = first.hand_back_in_person(&claim).unwrap();
        assert_eq!(phone.take_hand_back(&hand_back).unwrap(), 1);
    }
    for guardian in &lost.guardians[1..] {
        let hand_back = guardian.vauchi().hand_back_in_person(&claim).unwrap();
        phone.take_hand_back(&hand_back).unwrap();
    }

    phone.restore_identity_from_hand_backs(None).unwrap();

    assert_eq!(
        phone.identity().unwrap().signing_public_key(),
        lost.alice.identity().unwrap().signing_public_key()
    );
}
