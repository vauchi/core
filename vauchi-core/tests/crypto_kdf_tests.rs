// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for crypto::kdf
//! Extracted from kdf.rs

use vauchi_core::crypto::*;

// RFC 5869 Test Vectors for HKDF-SHA256

#[test]
fn test_hkdf_sha256_test_vector_1() {
    // Test Case 1 from RFC 5869
    let ikm = hex::decode("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b").unwrap();
    let salt = hex::decode("000102030405060708090a0b0c").unwrap();
    let info = hex::decode("f0f1f2f3f4f5f6f7f8f9").unwrap();
    let expected_prk =
        hex::decode("077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5").unwrap();
    let expected_okm = hex::decode(
        "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865",
    )
    .unwrap();

    let prk = HKDF::extract(Some(&salt), &ikm);
    assert_eq!(prk.as_slice(), expected_prk.as_slice());

    let okm = HKDF::expand(&prk, &info, 42).unwrap();
    assert_eq!(okm, expected_okm);
}

#[test]
fn test_hkdf_sha256_test_vector_2() {
    // Test Case 2 from RFC 5869 (longer inputs/outputs)
    let ikm = hex::decode(
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f\
             202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f\
             404142434445464748494a4b4c4d4e4f",
    )
    .unwrap();
    let salt = hex::decode(
        "606162636465666768696a6b6c6d6e6f707172737475767778797a7b7c7d7e7f\
             808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f\
             a0a1a2a3a4a5a6a7a8a9aaabacadaeaf",
    )
    .unwrap();
    let info = hex::decode(
        "b0b1b2b3b4b5b6b7b8b9babbbcbdbebfc0c1c2c3c4c5c6c7c8c9cacbcccdcecf\
             d0d1d2d3d4d5d6d7d8d9dadbdcdddedfe0e1e2e3e4e5e6e7e8e9eaebecedeeef\
             f0f1f2f3f4f5f6f7f8f9fafbfcfdfeff",
    )
    .unwrap();
    let expected_prk =
        hex::decode("06a6b88c5853361a06104c9ceb35b45cef760014904671014a193f40c15fc244").unwrap();
    let expected_okm = hex::decode(
        "b11e398dc80327a1c8e7f78c596a49344f012eda2d4efad8a050cc4c19afa97c\
             59045a99cac7827271cb41c65e590e09da3275600c2f09b8367793a9aca3db71\
             cc30c58179ec3e87c14c01d5c1f3434f1d87",
    )
    .unwrap();

    let prk = HKDF::extract(Some(&salt), &ikm);
    assert_eq!(prk.as_slice(), expected_prk.as_slice());

    let okm = HKDF::expand(&prk, &info, 82).unwrap();
    assert_eq!(okm, expected_okm);
}

#[test]
fn test_hkdf_sha256_test_vector_3() {
    // Test Case 3 from RFC 5869 (zero-length salt and info)
    let ikm = hex::decode("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b").unwrap();
    let expected_prk =
        hex::decode("19ef24a32c717b167f33a91d6f648bdf96596776afdb6377ac434c1c293ccb04").unwrap();
    let expected_okm = hex::decode(
        "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
             9d201395faa4b61a96c8",
    )
    .unwrap();

    // salt = None (zero-length)
    let prk = HKDF::extract(None, &ikm);
    assert_eq!(prk.as_slice(), expected_prk.as_slice());

    // info = empty
    let okm = HKDF::expand(&prk, &[], 42).unwrap();
    assert_eq!(okm, expected_okm);
}

#[test]
fn test_hkdf_derive_convenience() {
    let ikm = b"input key material";
    let salt = b"salt";
    let info = b"context info";

    let result = HKDF::derive(Some(salt), ikm, info, 64).unwrap();
    assert_eq!(result.len(), 64);

    // Should be deterministic
    let result2 = HKDF::derive(Some(salt), ikm, info, 64).unwrap();
    assert_eq!(result, result2);
}

#[test]
fn test_hkdf_derive_key() {
    let ikm = b"shared secret from X3DH";
    let info = b"Vauchi_Chain_Key";

    let key = HKDF::derive_key(None, ikm, info);
    assert_eq!(key.len(), 32);

    // Deterministic
    let key2 = HKDF::derive_key(None, ikm, info);
    assert_eq!(key, key2);
}

#[test]
fn test_hkdf_derive_key_pair() {
    let ikm = b"DH shared secret";
    let info = b"Vauchi_Root_Ratchet";

    let (key1, key2) = HKDF::derive_key_pair(None, ikm, info);

    // Both keys should be 32 bytes
    assert_eq!(key1.len(), 32);
    assert_eq!(key2.len(), 32);

    // Keys should be different
    assert_ne!(key1, key2);
}

#[test]
fn test_hkdf_output_too_long() {
    let prk = [0u8; 32];
    let info = b"test";

    // Max is 255 * 32 = 8160
    let result = HKDF::expand(&prk, info, 8161);
    assert!(matches!(result, Err(KDFError::OutputTooLong)));
}

#[test]
fn test_hkdf_zero_length_output() {
    let prk = [0u8; 32];
    let info = b"test";

    let result = HKDF::expand(&prk, info, 0).unwrap();
    assert!(result.is_empty());
}

#[test]
fn test_hkdf_different_info_different_output() {
    let ikm = b"same input";

    let key1 = HKDF::derive_key(None, ikm, b"info1");
    let key2 = HKDF::derive_key(None, ikm, b"info2");

    assert_ne!(key1, key2);
}

#[test]
fn test_hkdf_different_salt_different_output() {
    let ikm = b"same input";
    let info = b"same info";

    let key1 = HKDF::derive_key(Some(b"salt1"), ikm, info);
    let key2 = HKDF::derive_key(Some(b"salt2"), ikm, info);

    assert_ne!(key1, key2);
}
