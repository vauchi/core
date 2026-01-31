// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::password_kdf

use vauchi_core::crypto::password_kdf::{
    derive_key_argon2id, derive_key_pbkdf2, derive_key_pbkdf2_default,
};

#[test]
fn test_argon2id_deterministic() {
    let password = b"correct-horse-battery-staple";
    let salt = b"random_salt_16b!";

    let key1 = derive_key_argon2id(password, salt).unwrap();
    let key2 = derive_key_argon2id(password, salt).unwrap();
    assert_eq!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_argon2id_different_passwords_different_keys() {
    let salt = b"same_salt_16byte";
    let key1 = derive_key_argon2id(b"password1", salt).unwrap();
    let key2 = derive_key_argon2id(b"password2", salt).unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_argon2id_different_salts_different_keys() {
    let password = b"same_password";
    let key1 = derive_key_argon2id(password, b"salt_one_16bytes").unwrap();
    let key2 = derive_key_argon2id(password, b"salt_two_16bytes").unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_pbkdf2_deterministic() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let key1 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    let key2 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    assert_eq!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_pbkdf2_different_iterations() {
    let password = b"test_password";
    let salt = b"test_salt_value!";

    let key1 = derive_key_pbkdf2(password, salt, 1000).unwrap();
    let key2 = derive_key_pbkdf2(password, salt, 2000).unwrap();
    assert_ne!(key1.as_bytes(), key2.as_bytes());
}

#[test]
fn test_pbkdf2_default_works() {
    let key = derive_key_pbkdf2_default(b"my_password", b"my_salt_value!!!").unwrap();
    assert_eq!(key.as_bytes().len(), 32);
}

#[test]
fn test_argon2id_produces_32_byte_key() {
    let key = derive_key_argon2id(b"pass", b"saltysaltysalty!!").unwrap();
    assert_eq!(key.as_bytes().len(), 32);
}

#[test]
fn test_argon2id_vs_pbkdf2_different() {
    let password = b"same_password";
    let salt = b"same_salt_16byte";

    let argon_key = derive_key_argon2id(password, salt).unwrap();
    let pbkdf2_key = derive_key_pbkdf2(password, salt, 100_000).unwrap();
    assert_ne!(argon_key.as_bytes(), pbkdf2_key.as_bytes());
}
