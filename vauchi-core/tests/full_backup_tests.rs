// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for full backup v3: identity + contacts + own card + labels in one
//! encrypted envelope.
//!
//! Verifies round-trip fidelity, wrong-password rejection, corruption detection,
//! salt uniqueness, and backward compatibility with v2 identity backups.

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{
    BackupError, FullBackupIdentityData, ImportSource, export_contact_backup, export_full_backup,
    extract_master_seed, import_contact_backup, import_full_backup, restore_contacts_from_envelope,
};

// ── Helpers ────────────────────────────────────────────────────────────────

const PASSWORD: &str = "correct-horse-battery-staple";

fn test_identity_data() -> FullBackupIdentityData {
    FullBackupIdentityData {
        display_name: "Alice Tester".to_string(),
        master_seed: [0xAA; 32],
        device_index: 0,
        device_name: "Primary Device".to_string(),
    }
}

fn make_exchanged(name: &str) -> Contact {
    let mut pk = [0u8; 32];
    for (i, &b) in name.as_bytes().iter().enumerate() {
        pk[i % 32] ^= b;
    }
    pk[31] ^= 0xAB;
    let card = ContactCard::new(name);
    let key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, key)
}

fn make_imported(name: &str, source: ImportSource) -> Contact {
    let card = ContactCard::new(name);
    Contact::from_import(card, source, Some(format!("uid-{name}")))
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Round-trip with identity only (no contacts, no own card, no labels).
#[test]
fn v3_roundtrip_identity_only() {
    let id_data = test_identity_data();
    let blob = export_full_backup(&id_data, &[], None, &[], PASSWORD).unwrap();

    assert!(!blob.is_empty());
    assert_eq!(blob[0], 0x03, "version byte must be 0x03");

    let envelope = import_full_backup(&blob, PASSWORD).unwrap();
    assert_eq!(envelope.version, 3);
    assert_eq!(envelope.sections.identity.display_name, "Alice Tester");
    assert_eq!(envelope.sections.identity.device_index, 0);
    assert_eq!(envelope.sections.identity.device_name, "Primary Device");

    let seed = extract_master_seed(&envelope.sections.identity).unwrap();
    assert_eq!(*seed, [0xAA; 32], "master seed must round-trip exactly");

    assert!(envelope.sections.contacts.is_empty());
    assert!(envelope.sections.own_card.is_none());
    assert!(envelope.sections.labels.is_empty());
}

/// Round-trip with mixed contacts (exchanged + imported).
#[test]
fn v3_roundtrip_with_contacts() {
    let id_data = test_identity_data();
    let alice = make_exchanged("Alice");
    let bob = make_imported("Bob", ImportSource::VcardFile);
    let carol = make_exchanged("Carol");

    let blob = export_full_backup(
        &id_data,
        &[alice.clone(), bob.clone(), carol.clone()],
        None,
        &[],
        PASSWORD,
    )
    .unwrap();

    let envelope = import_full_backup(&blob, PASSWORD).unwrap();
    assert_eq!(envelope.sections.contacts.len(), 3);

    let restored = restore_contacts_from_envelope(&envelope).unwrap();
    assert_eq!(restored.len(), 3);

    assert_eq!(restored[0].id(), alice.id());
    assert_eq!(restored[0].display_name(), alice.display_name());
    assert!(restored[0].is_exchanged());

    assert_eq!(restored[1].id(), bob.id());
    assert_eq!(restored[1].display_name(), bob.display_name());
    assert!(restored[1].is_imported());

    assert_eq!(restored[2].id(), carol.id());
    assert_eq!(restored[2].display_name(), carol.display_name());
    assert!(restored[2].is_exchanged());
}

/// Own card survives the round-trip.
#[test]
fn v3_roundtrip_with_own_card() {
    let id_data = test_identity_data();
    let own_card = ContactCard::new("My Own Card");

    let blob = export_full_backup(&id_data, &[], Some(&own_card), &[], PASSWORD).unwrap();
    let envelope = import_full_backup(&blob, PASSWORD).unwrap();

    let restored_card = envelope
        .sections
        .own_card
        .as_ref()
        .expect("own_card must be present");
    assert_eq!(
        restored_card.display_name(),
        own_card.display_name(),
        "own card display name must round-trip"
    );
}

/// Labels survive the round-trip.
#[test]
fn v3_roundtrip_with_labels() {
    let id_data = test_identity_data();
    let labels = vec![
        (
            "lbl-1".to_string(),
            "Family".to_string(),
            vec!["c1".to_string(), "c2".to_string()],
        ),
        (
            "lbl-2".to_string(),
            "Work".to_string(),
            vec!["c3".to_string()],
        ),
    ];

    let blob = export_full_backup(&id_data, &[], None, &labels, PASSWORD).unwrap();
    let envelope = import_full_backup(&blob, PASSWORD).unwrap();

    assert_eq!(envelope.sections.labels.len(), 2);
    assert_eq!(envelope.sections.labels[0].label_id, "lbl-1");
    assert_eq!(envelope.sections.labels[0].name, "Family");
    assert_eq!(envelope.sections.labels[0].contacts, vec!["c1", "c2"]);
    assert_eq!(envelope.sections.labels[1].label_id, "lbl-2");
    assert_eq!(envelope.sections.labels[1].name, "Work");
    assert_eq!(envelope.sections.labels[1].contacts, vec!["c3"]);
}

/// Wrong password must return DecryptionFailed.
#[test]
fn v3_wrong_password_fails() {
    let id_data = test_identity_data();
    let blob = export_full_backup(&id_data, &[], None, &[], PASSWORD).unwrap();

    let result = import_full_backup(&blob, "wrong-password-here!");
    assert!(
        matches!(result, Err(BackupError::DecryptionFailed)),
        "expected DecryptionFailed, got {result:?}"
    );
}

/// Flipping a byte in the ciphertext must fail (AEAD integrity).
#[test]
fn v3_corrupted_data_fails() {
    let id_data = test_identity_data();
    let mut blob = export_full_backup(&id_data, &[], None, &[], PASSWORD).unwrap();

    let tamper_index = 1 + 16 + 4; // inside the ciphertext
    assert!(blob.len() > tamper_index);
    blob[tamper_index] ^= 0xFF;

    let result = import_full_backup(&blob, PASSWORD);
    assert!(
        matches!(result, Err(BackupError::DecryptionFailed)),
        "expected DecryptionFailed after tampering, got {result:?}"
    );
}

/// Two exports with the same data and password produce different ciphertext
/// (because each uses a fresh random salt).
#[test]
fn v3_different_salt_different_ciphertext() {
    let id_data1 = test_identity_data();
    let id_data2 = test_identity_data();
    let blob1 = export_full_backup(&id_data1, &[], None, &[], PASSWORD).unwrap();
    let blob2 = export_full_backup(&id_data2, &[], None, &[], PASSWORD).unwrap();

    // Salts (bytes 1..17) should differ
    assert_ne!(
        &blob1[1..17],
        &blob2[1..17],
        "two exports must have different salts"
    );
    // Overall ciphertext differs
    assert_ne!(blob1, blob2, "two exports must produce different blobs");

    // Both must still import correctly
    let env1 = import_full_backup(&blob1, PASSWORD).unwrap();
    let env2 = import_full_backup(&blob2, PASSWORD).unwrap();
    assert_eq!(env1.sections.identity.display_name, "Alice Tester");
    assert_eq!(env2.sections.identity.display_name, "Alice Tester");
}

/// v2 identity backup still imports via existing code (no regression).
#[test]
fn v2_backward_compat() {
    let identity = vauchi_core::Identity::create("V2 Compat Test");
    let password = "SecureP@ssw0rd!2024";
    let backup = identity.export_backup(password).unwrap();

    assert_eq!(
        backup.as_bytes()[0],
        0x02,
        "identity backup must use v2 format"
    );

    let restored = vauchi_core::Identity::import_backup(&backup, password).unwrap();
    assert_eq!(restored.public_id(), identity.public_id());
    assert_eq!(restored.display_name(), identity.display_name());
}

/// v1 contact backup still imports via existing code (no regression).
#[test]
fn v1_contact_backward_compat() {
    let contact = make_exchanged("BackwardCompat");
    let blob = export_contact_backup(&[contact.clone()], PASSWORD).unwrap();

    assert_eq!(blob[0], 0x01, "contact backup must use v1 format");

    let restored = import_contact_backup(&blob, PASSWORD).unwrap();
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].id(), contact.id());
}

/// Truncated data returns TooShort.
#[test]
fn v3_truncated_data_fails() {
    let result = import_full_backup(&[0x03, 0x01], PASSWORD);
    assert!(
        matches!(result, Err(BackupError::TooShort)),
        "expected TooShort, got {result:?}"
    );
}

/// Unknown version byte returns UnsupportedVersion.
#[test]
fn v3_wrong_version_fails() {
    let mut fake = vec![0xFFu8];
    fake.extend_from_slice(&[0u8; 100]);
    let result = import_full_backup(&fake, PASSWORD);
    assert!(
        matches!(result, Err(BackupError::UnsupportedVersion(0xFF))),
        "expected UnsupportedVersion(0xFF), got {result:?}"
    );
}
