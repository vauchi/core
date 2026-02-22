// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::password_kdf

#![allow(deprecated)]

use vauchi_core::crypto::password_kdf::{
    derive_key_argon2id, derive_key_pbkdf2, derive_key_pbkdf2_compat, derive_key_pbkdf2_default,
};

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_argon2id_deterministic() {
    let password = b"correct-horse-battery-staple";
    let salt = b"random_salt_16b!";

    let key1 = derive_key_argon2id(password, salt).unwrap();
    let key2 = derive_key_argon2id(password, salt).unwrap();
    assert_eq!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_argon2id_different_passwords_different_keys() {
    let salt = b"same_salt_16byte";
    let key1 = derive_key_argon2id(b"password1", salt).unwrap();
    let key2 = derive_key_argon2id(b"password2", salt).unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_argon2id_different_salts_different_keys() {
    let password = b"same_password";
    let key1 = derive_key_argon2id(password, b"salt_one_16bytes").unwrap();
    let key2 = derive_key_argon2id(password, b"salt_two_16bytes").unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_pbkdf2_deterministic() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let key1 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    let key2 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    assert_eq!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_pbkdf2_different_iterations() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let key1 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    let key2 = derive_key_pbkdf2(password, salt, 2000).unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

// @scenario: identity_management.feature:Create encrypted identity backup
#[test]
fn test_pbkdf2_default_works() {
    let key = derive_key_pbkdf2_default(b"my_password", b"my_salt_value!!!").unwrap();
    assert_eq!(key.as_bytes().len(), 32);
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_argon2id_produces_32_byte_key() {
    let key = derive_key_argon2id(b"pass", b"saltysaltysalty!!").unwrap();
    assert_eq!(key.as_bytes().len(), 32);
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_argon2id_vs_pbkdf2_different() {
    let password = b"same_password";
    let salt = b"same_salt_16byte";

    let argon_key = derive_key_argon2id(password, salt).unwrap();
    let pbkdf2_key = derive_key_pbkdf2(password, salt, 100_000).unwrap();
    assert_ne!(argon_key.as_bytes(), pbkdf2_key.as_bytes());
}

// @scenario: security.feature:Brute force protection on backup password
#[test]
fn test_pbkdf2_default_uses_600k_iterations() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let default_key = derive_key_pbkdf2_default(password, salt).unwrap();
    let explicit_600k = derive_key_pbkdf2(password, salt, 600_000).unwrap();
    let explicit_100k = derive_key_pbkdf2(password, salt, 100_000).unwrap();

    // Default should now match 600K, not 100K
    assert_eq!(default_key.as_bytes(), explicit_600k.as_bytes());
    assert_ne!(default_key.as_bytes(), explicit_100k.as_bytes());
}

// @scenario: identity_management.feature:Restore identity from backup
#[test]
fn test_pbkdf2_compat_returns_both_keys() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let keys = derive_key_pbkdf2_compat(password, salt).unwrap();
    assert_eq!(keys.len(), 2);

    let explicit_600k = derive_key_pbkdf2(password, salt, 600_000).unwrap();
    let explicit_100k = derive_key_pbkdf2(password, salt, 100_000).unwrap();

    // First key is 600K (modern), second is 100K (legacy)
    assert_eq!(keys[0].as_bytes(), explicit_600k.as_bytes());
    assert_eq!(keys[1].as_bytes(), explicit_100k.as_bytes());
}

// @scenario: identity_management.feature:Restore identity from backup
#[test]
fn test_pbkdf2_compat_legacy_backup_decryptable() {
    use vauchi_core::crypto::{decrypt, encrypt};

    let password = b"backup_password";
    let salt = b"backup_salt_16b!";

    // Simulate a legacy backup encrypted with 100K iterations
    let legacy_key = derive_key_pbkdf2(password, salt, 100_000).unwrap();
    let plaintext = b"secret identity data";
    let ciphertext = encrypt(&legacy_key, plaintext).unwrap();

    // The compat function should produce a key that can decrypt it
    let candidate_keys = derive_key_pbkdf2_compat(password, salt).unwrap();

    let mut decrypted = false;
    for key in &candidate_keys {
        if let Ok(result) = decrypt(key, &ciphertext) {
            assert_eq!(result, plaintext);
            decrypted = true;
            break;
        }
    }
    assert!(
        decrypted,
        "Legacy backup should be decryptable via compat keys"
    );
}
