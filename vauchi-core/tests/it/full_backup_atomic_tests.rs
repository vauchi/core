// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A failed full import must leave both storage and memory uninitialized.

use std::io::Write;

use flate2::{Compression, write::ZlibEncoder};
use vauchi_core::Vauchi;
use vauchi_core::backup::{FullBackupEnvelope, FullBackupIdentityData};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::{HKDF, SymmetricKey, derive_key_argon2id, encrypt};
use zeroize::Zeroizing;

const PASSWORD: &str = "correct-horse-battery-staple";

pub(super) fn legacy_backup() -> Vec<u8> {
    let mut source = Vauchi::in_memory().unwrap();
    source.create_identity("Alice").unwrap();
    let identity = source.identity().unwrap();
    let data = FullBackupIdentityData {
        display_name: identity.display_name().into(),
        master_seed: *identity.master_seed(),
        device_index: identity.device_index(),
        device_name: identity.device_info().device_name().into(),
    };
    let bob = Contact::from_exchange(
        [0x42; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        10,
    );
    vauchi_core::backup::export_full_backup(
        &data,
        &[bob.clone()],
        Some(&ContactCard::new("Alice")),
        &[("work".into(), "Work".into(), vec![bob.id().into()])],
        PASSWORD,
        20,
    )
    .unwrap()
}

pub(super) fn encode_legacy(envelope: &FullBackupEnvelope) -> String {
    let plaintext = Zeroizing::new(serde_json::to_vec(envelope).unwrap());
    let mut compressor = ZlibEncoder::new(Vec::new(), Compression::fast());
    compressor.write_all(&plaintext).unwrap();
    let compressed = Zeroizing::new(compressor.finish().unwrap());
    let salt = [0x24; 16];
    let base = derive_key_argon2id(PASSWORD.as_bytes(), &salt).unwrap();
    let prk = HKDF::extract(None, base.as_bytes());
    let expanded = Zeroizing::new(HKDF::expand(&prk, b"vauchi-backup-v3", 32).unwrap());
    let key = SymmetricKey::try_from_bytes(expanded.as_slice().try_into().unwrap()).unwrap();
    let ciphertext = encrypt(&key, &compressed).unwrap();
    let mut blob = vec![3];
    blob.extend(salt);
    blob.extend(ciphertext);
    hex::encode(blob)
}

fn assert_fresh(target: &Vauchi) {
    assert!(target.identity().is_none());
    assert!(
        target
            .storage()
            .identity()
            .load_identity()
            .unwrap()
            .is_none()
    );
    assert!(
        target
            .storage()
            .contacts()
            .load_own_card()
            .unwrap()
            .is_none()
    );
    assert_eq!(target.storage().contacts().count_contacts().unwrap(), 0);
    assert!(
        target
            .storage()
            .labels()
            .load_all_groups()
            .unwrap()
            .is_empty()
    );
}

// @scenario: backup_format_versioning :: Full backup import is atomic
#[test]
fn full_backup_atomic_malformed_late_contact_leaves_no_partial_restore() {
    let mut envelope = vauchi_core::backup::import_full_backup(&legacy_backup(), PASSWORD).unwrap();
    envelope
        .sections
        .contacts
        .push(serde_json::json!({"id": "invalid"}));
    let mut target = Vauchi::in_memory().unwrap();

    assert!(
        target
            .import_full_backup(&encode_legacy(&envelope), PASSWORD)
            .is_err()
    );
    assert_fresh(&target);
    target
        .import_full_backup(&hex::encode(legacy_backup()), PASSWORD)
        .unwrap();
    assert_eq!(target.identity().unwrap().display_name(), "Alice");
    assert_eq!(target.storage().contacts().count_contacts().unwrap(), 1);
}

// @scenario: backup_format_versioning :: Full backup import is atomic
#[test]
fn full_backup_atomic_storage_failure_rolls_back_and_allows_retry() {
    let backup = hex::encode(legacy_backup());
    let mut target = Vauchi::in_memory().unwrap();
    target
        .storage()
        .connection()
        .execute_batch(
            "CREATE TRIGGER reject_restore_group BEFORE INSERT ON visibility_labels
         BEGIN SELECT RAISE(ABORT, 'injected group write failure'); END;",
        )
        .unwrap();

    assert!(target.import_full_backup(&backup, PASSWORD).is_err());
    assert_fresh(&target);
    target
        .storage()
        .connection()
        .execute_batch("DROP TRIGGER reject_restore_group")
        .unwrap();
    target.import_full_backup(&backup, PASSWORD).unwrap();
    assert_eq!(target.identity().unwrap().display_name(), "Alice");
    assert_eq!(
        target.storage().labels().load_all_groups().unwrap()[0].name(),
        "Work"
    );
}

// @scenario: backup_format_versioning :: Full backup import is atomic
#[test]
fn full_backup_atomic_commit_failure_rolls_back_and_allows_retry() {
    let backup = hex::encode(legacy_backup());
    let mut target = Vauchi::in_memory().unwrap();
    target.storage().arm_commit_fault();
    assert!(target.import_full_backup(&backup, PASSWORD).is_err());
    assert_fresh(&target);
    target.import_full_backup(&backup, PASSWORD).unwrap();
    assert_eq!(target.identity().unwrap().display_name(), "Alice");
    assert_eq!(target.storage().contacts().count_contacts().unwrap(), 1);
}

// @scenario: backup_format_versioning :: Full backup includes identity, contacts, own card, and labels
#[test]
fn full_backup_atomic_legacy_omitted_group_members_do_not_prevent_restore() {
    let mut envelope = vauchi_core::backup::import_full_backup(&legacy_backup(), PASSWORD).unwrap();
    envelope.sections.labels[0]
        .contacts
        .push("omitted-archived-contact".into());
    let mut target = Vauchi::in_memory().unwrap();
    target
        .import_full_backup(&encode_legacy(&envelope), PASSWORD)
        .unwrap();
    let contacts = target.storage().contacts().list_contacts().unwrap();
    assert_eq!(contacts.len(), 1);
    let group = target.storage().labels().load_group("work").unwrap();
    assert_eq!(
        group.contacts(),
        &std::collections::HashSet::from([contacts[0].id().to_owned()])
    );
}
