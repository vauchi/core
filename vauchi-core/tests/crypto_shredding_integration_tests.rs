// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crypto Shredding Integration Tests (Item 207)
//!
//! Tests that key destruction renders data irrecoverable, exercising
//! the full SMK → SEK/FKEK derivation chain and ShredManager.

use std::sync::Arc;

use vauchi_core::crypto::{decrypt, encrypt};
use vauchi_core::identity::Identity;
use vauchi_core::storage::{MemoryKeyStorage, SecureStorage, Storage};

const SMK_KEY_NAME: &str = "smk";

// ============================================================
// SMK destruction renders SEK-encrypted data irrecoverable
// ============================================================

/// After SMK is destroyed and re-derived from a different identity,
/// all prior encrypted data is irrecoverable (different key material).
#[test]
fn test_smk_destruction_makes_data_irrecoverable() {
    let identity = Identity::create("Alice");

    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    // Encrypt data with SEK
    let plaintext = b"Sensitive contact data";
    let ciphertext = encrypt(&sek, plaintext).unwrap();

    // Recoverable with same SEK
    let recovered = decrypt(&sek, &ciphertext).unwrap();
    assert_eq!(recovered, plaintext);

    // Different identity → different master seed → different SMK → different SEK
    let different_identity = Identity::create("Alice");
    let different_smk = different_identity.derive_smk();
    let different_sek = different_smk.derive_sek();

    // Data encrypted under original SEK must be unrecoverable
    let result = decrypt(&different_sek, &ciphertext);
    assert!(
        result.is_err(),
        "Data must be irrecoverable with different SEK"
    );
}

// ============================================================
// FKEK derivation chain
// ============================================================

/// FKEK encrypts file keys; after SMK destruction those file keys
/// are irrecoverable.
#[test]
fn test_fkek_protects_file_keys() {
    let identity = Identity::create("Alice");
    let smk = identity.derive_smk();
    let fkek = smk.derive_fkek();

    // Encrypt a file key with FKEK
    let file_key = [0x42u8; 32];
    let encrypted_file_key = encrypt(&fkek, &file_key).unwrap();

    // Recoverable with same FKEK
    let recovered = decrypt(&fkek, &encrypted_file_key).unwrap();
    assert_eq!(recovered, file_key.as_slice());

    // Different identity → different FKEK → irrecoverable
    let different_identity = Identity::create("Alice");
    let different_fkek = different_identity.derive_smk().derive_fkek();

    let result = decrypt(&different_fkek, &encrypted_file_key);
    assert!(
        result.is_err(),
        "File key must be irrecoverable with different FKEK"
    );
}

// ============================================================
// Key hierarchy produces distinct keys
// ============================================================

/// All derived keys in the hierarchy must be distinct.
#[test]
fn test_key_hierarchy_produces_distinct_keys() {
    let identity = Identity::create("Alice");
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();
    let fkek = smk.derive_fkek();

    assert_ne!(
        smk.as_bytes(),
        sek.as_bytes(),
        "SMK and SEK must be different"
    );
    assert_ne!(
        smk.as_bytes(),
        fkek.as_bytes(),
        "SMK and FKEK must be different"
    );
    assert_ne!(
        sek.as_bytes(),
        fkek.as_bytes(),
        "SEK and FKEK must be different"
    );
}

// ============================================================
// SMK stored and loaded from SecureStorage produces same SEK
// ============================================================

/// Full lifecycle: derive SMK → store → load → derive SEK → decrypt.
#[test]
fn test_smk_secure_storage_lifecycle() {
    let identity = Identity::create("Alice");

    // Phase 1: Create identity, derive and store SMK
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    let secure = Arc::new(MemoryKeyStorage::new());
    secure.save_key(SMK_KEY_NAME, smk.as_bytes()).unwrap();

    // Encrypt data with SEK
    let plaintext = b"Protected data";
    let ciphertext = encrypt(&sek, plaintext).unwrap();

    // Phase 2: Simulate app restart — load SMK from SecureStorage
    let loaded_bytes = secure.load_key(SMK_KEY_NAME).unwrap().unwrap();
    let loaded_smk_bytes: [u8; 32] = loaded_bytes.try_into().unwrap();
    let loaded_smk = vauchi_core::crypto::ShreddingMasterKey::from_bytes(loaded_smk_bytes);
    let loaded_sek = loaded_smk.derive_sek();

    // Data must be recoverable with loaded SEK
    let recovered = decrypt(&loaded_sek, &ciphertext).unwrap();
    assert_eq!(
        recovered, plaintext,
        "SEK from loaded SMK must decrypt data"
    );

    // Phase 3: Delete SMK from SecureStorage (simulate shred)
    secure.delete_key(SMK_KEY_NAME).unwrap();

    // SMK must be absent
    assert!(
        secure.load_key(SMK_KEY_NAME).unwrap().is_none(),
        "SMK must be absent after deletion"
    );
}

// ============================================================
// SEK opens Storage, destruction prevents reopening
// ============================================================

/// Storage opened with SEK works; different SEK cannot read the data.
#[test]
fn test_storage_keyed_by_sek() {
    let identity = Identity::create("Alice");
    let smk = identity.derive_smk();
    let sek = smk.derive_sek();

    // Open storage with SEK
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("vauchi.db");
    let storage = Storage::open(&db_path, sek).unwrap();

    // Save identity to storage
    let backup_data = identity.signing_public_key().to_vec();
    storage
        .save_identity(&backup_data, identity.display_name())
        .unwrap();

    // Load back
    let loaded = storage.load_identity().unwrap();
    assert!(loaded.is_some(), "Identity should be loadable");
    let (loaded_data, _) = loaded.unwrap();
    assert_eq!(loaded_data, backup_data, "Loaded data should match saved");

    // Different SEK cannot read the same data correctly
    let different_identity = Identity::create("Alice");
    let different_sek = different_identity.derive_smk().derive_sek();

    // Opening with wrong key: encrypted data cannot be decrypted
    let storage2 = Storage::open(&db_path, different_sek).unwrap();
    let loaded2 = storage2.load_identity();

    // Either fails to load (decryption error) or loads garbled data
    if let Ok(Some((data, _))) = loaded2 {
        assert_ne!(
            data, backup_data,
            "Wrong SEK should not produce correct data"
        );
    }
    // If it errors, that's also correct — data is not recoverable
}
