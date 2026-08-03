// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for full backup wiring through the Vauchi API.
//!
//! These tests verify that `export_full_backup_api()` / `import_full_backup_api()`
//! on the Vauchi struct correctly orchestrate data gathering from storage,
//! encryption, and restoration — the missing wiring layer.

use proptest::prelude::*;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{ContactField, FieldType, ImportSource, Vauchi, VauchiConfig};

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

// ── Guardian key shard backup E2E (ciphertext-only re-seal flow) ────────────
//
// A guardian is a full Vauchi identity. It opens the share sealed to it and
// re-seals it to the recovering party via `respond_to_recovery` — no guardian
// secret or plaintext Shamir share ever crosses the API boundary
// (problem 2026-07-13-mobile-guardian-backup-integration).

/// A guardian identity that can open and re-seal shares in Core.
fn guardian(name: &str) -> Vauchi {
    let mut v = Vauchi::in_memory().unwrap();
    v.create_identity(name).unwrap();
    v
}

/// The Ed25519 signing key a guardian entry is addressed to.
fn signing_pk(v: &Vauchi) -> [u8; 32] {
    *v.identity().unwrap().signing_public_key()
}

/// End-to-end guardian backup: export with 2-of-3 shards, two guardians
/// re-seal their shares to a fresh recovering identity, recover, and verify the
/// decrypted envelope matches — with no raw key material at the API boundary.
// @scenario: backup_format_versioning :: Guardian backup round-trip with shards
#[test]
fn guardian_backup_reseal_roundtrip() {
    let alice = setup_vauchi_with_data();
    let original_public_id = alice.public_id().unwrap();

    let guardians = [guardian("G0"), guardian("G1"), guardian("G2")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    assert!(!backup_hex.is_empty());
    assert_eq!(
        hex::decode(&backup_hex).unwrap()[0],
        0x04,
        "guardian backup must use v4 format byte"
    );
    assert_eq!(
        sealed_shares.len(),
        3,
        "expected one sealed share per guardian"
    );

    // Fresh identity on the recovering device.
    let mut recovering = Vauchi::in_memory().unwrap();
    recovering.create_identity("Alice Recovered").unwrap();
    let recovering_pk = signing_pk(&recovering);

    // Guardians 0 and 2 re-seal their shares to the recovering party.
    let re0 = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &recovering_pk)
        .unwrap();
    let re2 = guardians[2]
        .respond_to_recovery(&sealed_shares[2], &recovering_pk)
        .unwrap();

    // Ciphertext-only boundary: a re-seal is not the original sealed share.
    assert_ne!(
        re0, sealed_shares[0],
        "re-sealing to a new recipient must produce different ciphertext"
    );

    let envelope = recovering
        .recover_guardian_backup(&backup_hex, &[re0, re2], 2)
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
fn recover_rejects_insufficient_shares() {
    let alice = setup_vauchi_with_data();
    let guardians = [guardian("G0"), guardian("G1"), guardian("G2")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    let mut recovering = Vauchi::in_memory().unwrap();
    recovering.create_identity("Alice Recovered").unwrap();
    let recovering_pk = signing_pk(&recovering);

    let re0 = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &recovering_pk)
        .unwrap();

    // One share against a 2-of-3 threshold: reconstruction must fail.
    let result = recovering.recover_guardian_backup(&backup_hex, &[re0], 2);
    assert!(result.is_err(), "recovery with 1-of-2 threshold must fail");
}

/// A guardian cannot open a share sealed to a *different* guardian, so it
/// cannot re-seal one it was not designated for.
// @scenario: backup_format_versioning :: Guardian sealed share rejects wrong recipient
#[test]
fn respond_to_recovery_rejects_foreign_share() {
    let alice = setup_vauchi_with_data();
    let guardians = [guardian("G0"), guardian("G1")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (_backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    let recovering_pk = signing_pk(&guardian("Recovering"));

    // Guardian 1 tries to respond with guardian 0's share.
    let result = guardians[1].respond_to_recovery(&sealed_shares[0], &recovering_pk);
    assert!(
        result.is_err(),
        "a guardian must not open a share sealed to another identity"
    );
}

/// The recovering party cannot open shares that guardians re-sealed to a
/// different recipient — the re-seal is bound to the intended recovering key.
// @scenario: backup_format_versioning :: Guardian re-seal rejects wrong recovering key
#[test]
fn recover_rejects_shares_resealed_to_another_party() {
    let alice = setup_vauchi_with_data();
    let guardians = [guardian("G0"), guardian("G1"), guardian("G2")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    let mut recovering = Vauchi::in_memory().unwrap();
    recovering.create_identity("Alice Recovered").unwrap();
    let impostor_pk = signing_pk(&guardian("Impostor"));

    // Guardians re-seal to the impostor, not to the recovering party.
    let re0 = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &impostor_pk)
        .unwrap();
    let re2 = guardians[2]
        .respond_to_recovery(&sealed_shares[2], &impostor_pk)
        .unwrap();

    let result = recovering.recover_guardian_backup(&backup_hex, &[re0, re2], 2);
    assert!(
        result.is_err(),
        "recovering party must not open shares re-sealed to another key"
    );
}

/// A tampered re-sealed share must be rejected (AEAD integrity).
// @scenario: backup_format_versioning :: Guardian re-seal rejects tampered ciphertext
#[test]
fn recover_rejects_tampered_share() {
    let alice = setup_vauchi_with_data();
    let guardians = [guardian("G0"), guardian("G1"), guardian("G2")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    let mut recovering = Vauchi::in_memory().unwrap();
    recovering.create_identity("Alice Recovered").unwrap();
    let recovering_pk = signing_pk(&recovering);

    let mut re0 = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &recovering_pk)
        .unwrap();
    let re2 = guardians[2]
        .respond_to_recovery(&sealed_shares[2], &recovering_pk)
        .unwrap();

    let last = re0.len() - 1;
    re0[last] ^= 0xFF;

    let result = recovering.recover_guardian_backup(&backup_hex, &[re0, re2], 2);
    assert!(result.is_err(), "tampered re-sealed share must be rejected");
}

/// Two copies of the same guardian's share must not satisfy the threshold —
/// duplicate Shamir indices carry no independent information.
// @scenario: backup_format_versioning :: Guardian recovery rejects duplicate shares
#[test]
fn recover_rejects_duplicate_share() {
    let alice = setup_vauchi_with_data();
    let guardians = [guardian("G0"), guardian("G1"), guardian("G2")];
    let guardian_pks: Vec<[u8; 32]> = guardians.iter().map(signing_pk).collect();

    let (backup_hex, sealed_shares) = alice
        .export_guardian_backup_with_shards(&guardian_pks, 2)
        .unwrap();

    let mut recovering = Vauchi::in_memory().unwrap();
    recovering.create_identity("Alice Recovered").unwrap();
    let recovering_pk = signing_pk(&recovering);

    let re0 = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &recovering_pk)
        .unwrap();
    let re0_again = guardians[0]
        .respond_to_recovery(&sealed_shares[0], &recovering_pk)
        .unwrap();

    let result = recovering.recover_guardian_backup(&backup_hex, &[re0, re0_again], 2);
    assert!(
        result.is_err(),
        "two shares from the same guardian must not reconstruct the key"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// `respond_to_recovery` must never panic on arbitrary bytes, and must
    /// reject anything that is not a valid share sealed to this guardian
    /// (DC-01/DC-02: bounded, fail-closed parse boundary).
    // @internal
    #[test]
    fn respond_to_recovery_rejects_arbitrary_input(
        sealed in prop::collection::vec(any::<u8>(), 0..256),
        recovering_pk: [u8; 32],
    ) {
        let g = guardian("Guardian");
        prop_assert!(g.respond_to_recovery(&sealed, &recovering_pk).is_err());
    }

    /// `recover_guardian_backup` must never panic on arbitrary re-sealed shares.
    // @internal
    #[test]
    fn recover_rejects_arbitrary_shares(
        a in prop::collection::vec(any::<u8>(), 0..256),
        b in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let mut recovering = Vauchi::in_memory().unwrap();
        recovering.create_identity("Recovering").unwrap();
        prop_assert!(recovering.recover_guardian_backup("00", &[a, b], 2).is_err());
    }
}
