// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for content integrity verification
//!
//! Scenarios from remote-content.feature:
//! - Verify content checksum before saving
//! - Reject content with mismatched checksum

use vauchi_core::content::{compute_checksum, verify_checksum, IntegrityError};

#[test]
fn test_compute_checksum() {
    let data = b"hello world";
    let checksum = compute_checksum(data);

    // SHA-256 of "hello world"
    assert!(checksum.starts_with("sha256:"));
    assert_eq!(
        checksum,
        "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
    );
}

#[test]
fn test_compute_checksum_empty() {
    let data = b"";
    let checksum = compute_checksum(data);

    // SHA-256 of empty string
    assert_eq!(
        checksum,
        "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}

#[test]
fn test_verify_checksum_valid() {
    let data = b"hello world";
    let checksum = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    assert!(verify_checksum(data, checksum).is_ok());
}

#[test]
fn test_verify_checksum_mismatch() {
    let data = b"hello world";
    let wrong_checksum = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

    let result = verify_checksum(data, wrong_checksum);
    assert!(result.is_err());

    if let Err(IntegrityError::ChecksumMismatch { expected, actual }) = result {
        assert_eq!(
            expected,
            "0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(
            actual,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    } else {
        panic!("Expected ChecksumMismatch error");
    }
}

#[test]
fn test_verify_checksum_invalid_format() {
    let data = b"hello world";

    // Missing sha256: prefix
    let invalid = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    let result = verify_checksum(data, invalid);
    assert!(matches!(result, Err(IntegrityError::InvalidFormat)));
}

#[test]
fn test_verify_checksum_wrong_algorithm_prefix() {
    let data = b"hello world";

    // Wrong algorithm prefix
    let wrong_algo = "md5:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
    let result = verify_checksum(data, wrong_algo);
    assert!(matches!(result, Err(IntegrityError::InvalidFormat)));
}

#[test]
fn test_checksum_roundtrip() {
    let data = b"test content for checksum verification";
    let checksum = compute_checksum(data);
    assert!(verify_checksum(data, &checksum).is_ok());
}

#[test]
fn test_checksum_binary_data() {
    // Test with binary data including null bytes
    let data: Vec<u8> = (0..=255).collect();
    let checksum = compute_checksum(&data);
    assert!(verify_checksum(&data, &checksum).is_ok());
}

#[test]
fn test_checksum_large_data() {
    // Test with 1MB of data
    let data = vec![0x42u8; 1024 * 1024];
    let checksum = compute_checksum(&data);
    assert!(verify_checksum(&data, &checksum).is_ok());
}
