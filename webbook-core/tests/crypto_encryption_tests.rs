//! Tests for crypto::encryption
//! Extracted from encryption.rs

use webbook_core::crypto::*;
use webbook_core::*;

#[test]
fn test_basic_roundtrip() {
    let key = SymmetricKey::generate();
    let data = b"test data";
    let encrypted = encrypt(&key, data).unwrap();
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(data.to_vec(), decrypted);
}

#[test]
fn test_empty_data() {
    let key = SymmetricKey::generate();
    let data = b"";
    let encrypted = encrypt(&key, data).unwrap();
    let decrypted = decrypt(&key, &encrypted).unwrap();
    assert_eq!(data.to_vec(), decrypted);
}
