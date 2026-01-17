//! Tests for crypto::signing
//! Extracted from signing.rs

use webbook_core::crypto::*;
use webbook_core::*;

#[test]
fn test_keypair_generation() {
    let kp = SigningKeyPair::generate();
    assert_eq!(kp.public_key().as_bytes().len(), 32);
}

#[test]
fn test_sign_verify() {
    let kp = SigningKeyPair::generate();
    let msg = b"test message";
    let sig = kp.sign(msg);
    assert!(kp.public_key().verify(msg, &sig));
}
