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

use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::sync::guardian_piece::GuardianDelivery;
use vauchi_core::{AuthMode, ContactField, FieldType, Storage, SymmetricKey, Vauchi, VauchiConfig};

use crate::common::helpers::create_vauchi_with_identity;
use crate::common::lost_phone::{Lost, alice_pk_hex, lost};
use crate::common::meeting::signing_pk;

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
        .save_recovery_mode(&[4; 32], &[6; 32])
        .unwrap();
    storage
        .recovery()
        .keep_hand_back(&[5; 32], b"hand-back", 7)
        .unwrap();

    storage.rekey(SymmetricKey::generate()).unwrap();

    assert_eq!(
        storage
            .recovery()
            .load_recovery_mode()
            .unwrap()
            .map(|(seed, target)| (*seed, target)),
        Some(([4u8; 32], [6u8; 32]))
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

/// Delivers every update `from` queued for `to`, as a relay hand-off would.
fn deliver_all(
    from: &Vauchi,
    to_id_in_from: &str,
    to: &Vauchi,
    from_id_in_to: &str,
) -> Vec<ReceiveOutcome> {
    let queued = from
        .storage()
        .pending()
        .get_pending_updates(to_id_in_from)
        .unwrap();
    queued
        .into_iter()
        .map(|update| {
            from.storage()
                .pending()
                .delete_pending_update(&update.id)
                .unwrap();
            process_single_card_update(
                to.identity().unwrap(),
                to.storage(),
                from_id_in_to,
                &update.payload,
            )
            .unwrap()
        })
        .collect()
}

/// After the restore Dave keeps Alice as she was: the restored phone runs
/// the device-registry handshake, Dave learns the new device and opens its
/// card update, and no rotation, rekey or new contact happens.
// @internal
#[test]
fn contacts_reconnect_with_the_restored_identity_without_a_rotation() {
    let lost = lost();
    let mut phone = handed_back(&lost, 3);
    phone.restore_identity_from_hand_backs(None).unwrap();
    let dave = lost.dave.vauchi();
    let dave_at_phone = lost.dave.id_at_sharer.clone();
    let alice_at_dave = lost.dave.sharer_id().to_string();
    let dave_contacts_before = dave.list_contacts().unwrap().len();
    let alice_before = dave.get_contact(&alice_at_dave).unwrap().unwrap();

    assert!(phone.queue_registry_pushes().unwrap() >= 1);
    for outcome in deliver_all(&phone, &dave_at_phone, dave, &alice_at_dave) {
        if let ReceiveOutcome::RegistryPushReceived(reply) = outcome {
            dave.queue_registry_ack(&reply).unwrap();
        }
    }
    deliver_all(dave, &alice_at_dave, &phone, &dave_at_phone);
    let restored_device = *phone.identity().unwrap().device_id();
    assert!(
        dave.storage()
            .device()
            .load_contact_device_registry(&alice_at_dave)
            .unwrap()
            .expect("Dave holds Alice's registry")
            .contains_device(&restored_device),
        "Dave learned the restored device"
    );

    let old_card = phone.storage().contacts().load_own_card().unwrap().unwrap();
    let field = ContactField::new(FieldType::Email, "Work", "alice@new.example", 0);
    let field_id = field.id().to_string();
    phone.add_own_field(field).unwrap();
    phone.set_own_field_public(&field_id).unwrap();
    let new_card = phone.storage().contacts().load_own_card().unwrap().unwrap();
    let copies = phone
        .prepare_card_updates_for_contact(&dave_at_phone, &old_card, &new_card)
        .unwrap();
    assert!(!copies.is_empty());
    for (_, payload) in copies {
        process_single_card_update(
            dave.identity().unwrap(),
            dave.storage(),
            &alice_at_dave,
            &payload,
        )
        .unwrap();
    }

    let alice_after = dave.get_contact(&alice_at_dave).unwrap().unwrap();
    assert_eq!(alice_after.public_key(), alice_before.public_key());
    assert_eq!(
        alice_after.shared_key().unwrap().as_bytes(),
        alice_before.shared_key().unwrap().as_bytes(),
        "no rotation: the relationship's key is the one Dave had"
    );
    assert!(
        alice_after
            .card()
            .fields()
            .iter()
            .any(|f| f.value() == "alice@new.example"),
        "Dave opened the restored phone's card update"
    );
    assert_eq!(dave.list_contacts().unwrap().len(), dave_contacts_before);
    assert!(dave.rekey_answer_deliveries().unwrap().is_empty());
    assert!(phone.rekey_offer_deliveries().unwrap().is_empty());
}
