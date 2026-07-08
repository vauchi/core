// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for full backup wiring through the Vauchi API.
//!
//! These tests verify that `export_full_backup_api()` / `import_full_backup_api()`
//! on the Vauchi struct correctly orchestrate data gathering from storage,
//! encryption, and restoration — the missing wiring layer.

use ed25519_dalek::SigningKey;
use rand_core::OsRng;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{ContactField, FieldType, ImportSource, Vauchi, VauchiConfig};
use x25519_dalek::StaticSecret;

const BACKUP_PASSWORD: &str = "correct-horse-battery-staple";

/// Helper: create a Vauchi instance with identity + contacts + own card.
fn setup_vauchi_with_data() -> Vauchi {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity("Alice Smith").unwrap();

    v.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
        0,
    ))
    .unwrap();
    v.add_own_field(ContactField::new(
        FieldType::Phone,
        "mobile",
        "+15559876543",
        0,
    ))
    .unwrap();

    let card_bob = ContactCard::new("Bob");
    let key_bob = SymmetricKey::generate();
    let bob = Contact::from_exchange([0xBB; 32], card_bob, key_bob, 0);
    v.update_contact(&bob).unwrap();

    let card_carol = ContactCard::new("Carol");
    let key_carol = SymmetricKey::generate();
    let carol = Contact::from_exchange([0xCC; 32], card_carol, key_carol, 0);
    v.update_contact(&carol).unwrap();

    let card_dave = ContactCard::new("Dave");
    let dave = Contact::from_import(
        "contact-dave".to_string(),
        card_dave,
        ImportSource::VcardFile,
        Some("uid-dave".into()),
        0,
    );
    v.update_contact(&dave).unwrap();

    v
}

// ── Round-trip through Vauchi API ──────────────────────────────────────────

/// Full backup via Vauchi API: export → import on fresh instance → verify all data.
// @scenario: backup_format_versioning :: Full backup round-trip via API
#[test]
fn full_backup_api_roundtrip() {
    let v = setup_vauchi_with_data();

    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();
    assert!(!backup_hex.is_empty());

    let mut v2 = Vauchi::in_memory().unwrap();
    v2.import_full_backup(&backup_hex, BACKUP_PASSWORD).unwrap();

    // Identity must be restored
    assert_eq!(v2.public_id().unwrap(), v.public_id().unwrap());
    assert!(v2.identity().is_some());

    // Own card must be restored with fields
    let restored_card = v2.own_card().unwrap().unwrap();
    assert_eq!(restored_card.fields().len(), 2);

    // Contacts must be restored
    let contacts = v2.list_contacts().unwrap();
    assert_eq!(contacts.len(), 3, "expected 3 contacts (Bob, Carol, Dave)");

    let exchanged_count = contacts.iter().filter(|c| c.is_exchanged()).count();
    let imported_count = contacts.iter().filter(|c| c.is_imported()).count();
    assert_eq!(exchanged_count, 2);
    assert_eq!(imported_count, 1);
}

/// The identity persisted by a v3 full-backup import must be loadable
/// by a fresh instance over the same storage — i.e. survive a process
/// restart without the backup password. Regression:
/// `import_full_backup` saved the identity in the user-password-
/// encrypted backup format, which no startup loader can decrypt; the
/// restored user was locked out on relaunch (device-verified, Pixel
/// 3a — `2026-06-11-restore-identity-unloadable-after-restart`).
// @scenario: backup_format_versioning :: Restored identity survives restart
#[test]
fn full_backup_import_survives_restart() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut v2 = Vauchi::new(config).unwrap();
        v2.import_full_backup(&backup_hex, BACKUP_PASSWORD).unwrap();
        assert!(v2.identity().is_some());
    }

    let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let mut v3 = Vauchi::new(config).unwrap();
    v3.refresh_identity_from_storage();
    assert!(
        v3.identity().is_some(),
        "restored identity must load after restart"
    );
    assert_eq!(v3.public_id().unwrap(), v.public_id().unwrap());
    assert_eq!(v3.list_contacts().unwrap().len(), 3);
}

/// Same restart-survival contract for the v2 identity-only import.
// @scenario: backup_format_versioning :: Restored identity survives restart
#[test]
fn identity_backup_import_survives_restart() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_backup(BACKUP_PASSWORD).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let storage_key = SymmetricKey::generate();
    let db_path = dir.path().join("vauchi.db");

    {
        let config =
            VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key.clone());
        let mut v2 = Vauchi::new(config).unwrap();
        v2.import_backup(&backup_hex, BACKUP_PASSWORD).unwrap();
        assert!(v2.identity().is_some());
    }

    let config = VauchiConfig::with_storage_path(&db_path).with_storage_key(storage_key);
    let mut v3 = Vauchi::new(config).unwrap();
    v3.refresh_identity_from_storage();
    assert!(
        v3.identity().is_some(),
        "imported identity must load after restart"
    );
    assert_eq!(v3.public_id().unwrap(), v.public_id().unwrap());
}

/// Full backup with wrong password must fail.
// @scenario: backup_format_versioning :: Full backup wrong password
#[test]
fn full_backup_api_wrong_password() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let mut v2 = Vauchi::in_memory().unwrap();
    let result = v2.import_full_backup(&backup_hex, "wrong-password!");
    assert!(result.is_err());
}

/// Full backup on uninitialized identity must fail.
// @scenario: backup_format_versioning :: Full backup requires identity
#[test]
fn full_backup_api_no_identity() {
    let v = Vauchi::in_memory().unwrap();
    let result = v.export_full_backup(BACKUP_PASSWORD);
    assert!(result.is_err());
}

/// Full backup with zero contacts still works (identity-only content).
// @scenario: backup_format_versioning :: Full backup with empty contacts
#[test]
fn full_backup_api_no_contacts() {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity("Solo User").unwrap();

    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let mut v2 = Vauchi::in_memory().unwrap();
    v2.import_full_backup(&backup_hex, BACKUP_PASSWORD).unwrap();

    assert_eq!(v2.public_id().unwrap(), v.public_id().unwrap());
    let contacts = v2.list_contacts().unwrap();
    assert!(contacts.is_empty());
}

/// Import full backup on instance that already has identity must fail.
// @scenario: backup_format_versioning :: Full backup import rejects existing identity
#[test]
fn full_backup_api_import_rejects_existing_identity() {
    let v = setup_vauchi_with_data();
    let backup_hex = v.export_full_backup(BACKUP_PASSWORD).unwrap();

    let mut v2 = Vauchi::in_memory().unwrap();
    v2.create_identity("Already Here").unwrap();

    let result = v2.import_full_backup(&backup_hex, BACKUP_PASSWORD);
    assert!(result.is_err(), "import should reject when identity exists");
}

// ── Guardian key shard backup E2E ──────────────────────────────────────────

/// A guardian keypair for testing: Ed25519 identity key plus the matching
/// X25519 key used to open sealed shares.
struct GuardianKeys {
    ed25519_pk: [u8; 32],
    x25519_sk: StaticSecret,
}

impl GuardianKeys {
    fn generate() -> Self {
        let signing_key = SigningKey::generate(&mut OsRng);
        let ed25519_pk = signing_key.verifying_key().to_bytes();
        let x25519_sk = StaticSecret::from(signing_key.to_scalar_bytes());
        Self {
            ed25519_pk,
            x25519_sk,
        }
    }
}

/// End-to-end guardian backup: create identity + data, export with 2-of-3 shards,
/// recover with any 2 guardians, verify the decrypted envelope matches.
// @scenario: backup_format_versioning :: Guardian backup round-trip with shards
#[test]
fn guardian_backup_with_shards_roundtrip() {
    let v = setup_vauchi_with_data();
    let original_public_id = v.public_id().unwrap();

    let guardians = [
        GuardianKeys::generate(),
        GuardianKeys::generate(),
        GuardianKeys::generate(),
    ];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(|g| g.ed25519_pk).collect();

    let (backup_hex, sealed_shares) = v
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    assert!(!backup_hex.is_empty());
    let backup_bytes = hex::decode(&backup_hex).unwrap();
    assert_eq!(
        backup_bytes[0], 0x04,
        "guardian backup must use v4 format byte"
    );
    assert_eq!(
        sealed_shares.len(),
        3,
        "expected one sealed share per guardian"
    );

    // Open shares 0 and 2 with the corresponding guardian X25519 secret keys.
    let recovery_pairs = vec![
        (sealed_shares[0].clone(), guardians[0].x25519_sk.clone()),
        (sealed_shares[2].clone(), guardians[2].x25519_sk.clone()),
    ];

    let envelope = v
        .recover_guardian_backup(&backup_hex, &recovery_pairs)
        .unwrap();

    // Identity round-tripped.
    assert_eq!(envelope.sections.identity.display_name, "Alice Smith");

    // Contacts round-tripped.
    assert_eq!(envelope.sections.contacts.len(), 3);

    // Own card round-tripped.
    let own_card = envelope
        .sections
        .own_card
        .as_ref()
        .expect("own card must be present");
    assert_eq!(own_card.fields().len(), 2);

    // Master seed can be extracted and yields the same public identity.
    let seed = vauchi_core::extract_master_seed(&envelope.sections.identity).unwrap();
    let restored_identity = vauchi_core::Identity::from_device_link(
        *seed,
        envelope.sections.identity.display_name.clone(),
        envelope.sections.identity.device_index,
        envelope.sections.identity.device_name.clone(),
        0,
    );
    assert_eq!(restored_identity.public_id(), original_public_id);
}

/// Recovery must fail when fewer than the threshold of shares is provided.
// @scenario: backup_format_versioning :: Guardian backup recovery rejects insufficient shares
#[test]
fn guardian_backup_recovery_rejects_insufficient_shares() {
    let v = setup_vauchi_with_data();
    let guardians = [
        GuardianKeys::generate(),
        GuardianKeys::generate(),
        GuardianKeys::generate(),
    ];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(|g| g.ed25519_pk).collect();

    let (backup_hex, sealed_shares) = v
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    // Only one share: reconstruction must fail.
    let recovery_pairs = vec![(sealed_shares[0].clone(), guardians[0].x25519_sk.clone())];
    let result = v.recover_guardian_backup(&backup_hex, &recovery_pairs);
    assert!(result.is_err(), "recovery with 1-of-2 threshold must fail");
}

/// A sealed share must not open with a different guardian's secret key.
// @scenario: backup_format_versioning :: Guardian sealed share rejects wrong secret key
#[test]
fn guardian_backup_share_rejects_wrong_key() {
    let v = setup_vauchi_with_data();
    let guardians = [GuardianKeys::generate(), GuardianKeys::generate()];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(|g| g.ed25519_pk).collect();

    let (_backup_hex, sealed_shares) = v
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    // Use guardian 1's secret to open guardian 0's share.
    let wrong_pair = vec![(sealed_shares[0].clone(), guardians[1].x25519_sk.clone())];
    let result = v.recover_guardian_backup("00", &wrong_pair);
    assert!(
        result.is_err(),
        "share must not open with wrong guardian key"
    );
}
