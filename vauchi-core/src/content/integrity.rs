// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content integrity verification using SHA-256 checksums
//!
//! All remote content is verified using SHA-256 checksums before being
//! saved to the local cache. This ensures content has not been tampered
//! with during transit.

use aws_lc_rs::digest::{Context, SHA256};
use thiserror::Error;

/// Verify SHA-256 checksum of content
///
/// # Arguments
/// * `data` - The content bytes to verify
/// * `expected` - Expected checksum in format "sha256:hexstring"
///
/// # Returns
/// * `Ok(())` if checksum matches
/// * `Err(IntegrityError)` if checksum doesn't match or format is invalid
///
/// # Example
/// ```
/// use vauchi_core::content::{verify_checksum, compute_checksum};
///
/// let data = b"hello world";
/// let checksum = compute_checksum(data);
/// assert!(verify_checksum(data, &checksum).is_ok());
/// ```
pub fn verify_checksum(data: &[u8], expected: &str) -> Result<(), IntegrityError> {
    // Expected format: "sha256:hexstring"
    let expected_hex = expected
        .strip_prefix("sha256:")
        .ok_or(IntegrityError::InvalidFormat)?;

    let mut context = Context::new(&SHA256);
    context.update(data);
    let digest = context.finish();
    let actual_hex = hex::encode(digest.as_ref());

    if actual_hex == expected_hex {
        Ok(())
    } else {
        Err(IntegrityError::ChecksumMismatch {
            expected: expected_hex.to_string(),
            actual: actual_hex,
        })
    }
}

/// Compute SHA-256 checksum of content
///
/// # Arguments
/// * `data` - The content bytes to hash
///
/// # Returns
/// Checksum string in format "sha256:hexstring"
///
/// # Example
/// ```
/// use vauchi_core::content::compute_checksum;
///
/// let data = b"hello world";
/// let checksum = compute_checksum(data);
/// assert!(checksum.starts_with("sha256:"));
/// ```
pub fn compute_checksum(data: &[u8]) -> String {
    let mut context = Context::new(&SHA256);
    context.update(data);
    let digest = context.finish();
    format!("sha256:{}", hex::encode(digest.as_ref()))
}

/// Verify an Ed25519 signature on a content manifest.
///
/// The signed data is the canonical JSON of the manifest with the `signature` field removed.
/// This matches the signing process: serialize the manifest without the signature, sign,
/// then attach the signature.
///
/// # Arguments
/// * `manifest` - The manifest to verify (must have a `signature` field set)
/// * `public_key` - The publisher's Ed25519 public key (32 bytes)
///
/// # Returns
/// * `Ok(())` if the signature is valid
/// * `Err(IntegrityError)` if the signature is missing, malformed, or invalid
///
/// # Note
/// This function is available for future use once the CI signing infrastructure is in place.
/// Currently no code path gates updates on manifest signatures.
pub fn verify_manifest_signature(
    manifest: &super::types::ContentManifest,
    public_key: &crate::crypto::signing::PublicKey,
) -> Result<(), IntegrityError> {
    let sig_hex = manifest
        .signature
        .as_ref()
        .ok_or(IntegrityError::MissingSignature)?;

    // Decode hex signature (128 hex chars = 64 bytes)
    let sig_bytes: Vec<u8> = (0..sig_hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&sig_hex[i..i + 2], 16))
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|_| IntegrityError::InvalidSignatureFormat)?;

    if sig_bytes.len() != 64 {
        return Err(IntegrityError::InvalidSignatureFormat);
    }

    let signature = crate::crypto::signing::Signature::from_bytes(
        sig_bytes
            .try_into()
            .map_err(|_| IntegrityError::InvalidSignatureFormat)?,
    );

    // Compute canonical signed data: manifest JSON without the signature field
    let mut manifest_for_signing = manifest.clone();
    manifest_for_signing.signature = None;
    let canonical_json = serde_json::to_vec(&manifest_for_signing)
        .map_err(|e| IntegrityError::SignatureVerificationFailed(e.to_string()))?;

    if public_key.verify(&canonical_json, &signature) {
        Ok(())
    } else {
        Err(IntegrityError::SignatureVerificationFailed(
            "Ed25519 signature verification failed".to_string(),
        ))
    }
}

/// Errors that can occur during integrity verification
#[derive(Debug, Error)]
pub enum IntegrityError {
    /// Checksum format is invalid (missing "sha256:" prefix)
    #[error("Invalid checksum format, expected 'sha256:...'")]
    InvalidFormat,

    /// Computed checksum doesn't match expected checksum
    #[error("Checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected checksum (hex string without prefix)
        expected: String,
        /// Actual computed checksum (hex string without prefix)
        actual: String,
    },

    /// Manifest has no signature field
    #[error("Manifest signature is missing")]
    MissingSignature,

    /// Signature format is invalid (not valid hex or wrong length)
    #[error("Invalid signature format")]
    InvalidSignatureFormat,

    /// Signature verification failed
    #[error("Signature verification failed: {0}")]
    SignatureVerificationFailed(String),
}

// INLINE_TEST_REQUIRED: tests access private internals
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_hash() {
        // Known SHA-256 hash of "hello world"
        let data = b"hello world";
        let expected = "sha256:b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(verify_checksum(data, expected).is_ok());
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_valid() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use crate::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        // Create manifest without signature
        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://vauchi.app/app-files".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        // Sign: canonical JSON of manifest without signature
        let canonical_json = serde_json::to_vec(&manifest).unwrap();
        let signature = keypair.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Verify
        assert!(verify_manifest_signature(&manifest, &public_key).is_ok());
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_tampered() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use crate::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://vauchi.app/app-files".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        // Sign the original
        let canonical_json = serde_json::to_vec(&manifest).unwrap();
        let signature = keypair.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Tamper with the manifest
        manifest.base_url = "https://evil.example.com/files".to_string();

        // Verification should fail
        assert!(verify_manifest_signature(&manifest, &public_key).is_err());
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_missing() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use crate::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        let manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://vauchi.app/app-files".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        let result = verify_manifest_signature(&manifest, &public_key);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntegrityError::MissingSignature
        ));
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_wrong_key() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use crate::crypto::signing::SigningKeyPair;

        let signer = SigningKeyPair::generate();
        let wrong_key = SigningKeyPair::generate().public_key();

        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://vauchi.app/app-files".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        let canonical_json = serde_json::to_vec(&manifest).unwrap();
        let signature = signer.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Verification with wrong key should fail
        assert!(verify_manifest_signature(&manifest, &wrong_key).is_err());
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_manifest_signature_field_backward_compatible() {
        // Ensure manifests without signature field still deserialize
        let json = r#"{
            "schema_version": 1,
            "generated_at": "2026-01-01T00:00:00Z",
            "base_url": "https://vauchi.app/files",
            "content": {}
        }"#;

        let manifest: crate::content::types::ContentManifest = serde_json::from_str(json).unwrap();
        assert!(manifest.signature.is_none());

        // And manifests with signature field also deserialize
        let json_with_sig = r#"{
            "schema_version": 1,
            "generated_at": "2026-01-01T00:00:00Z",
            "base_url": "https://vauchi.app/files",
            "content": {},
            "signature": "abcd1234"
        }"#;

        let manifest: crate::content::types::ContentManifest =
            serde_json::from_str(json_with_sig).unwrap();
        assert_eq!(manifest.signature.as_deref(), Some("abcd1234"));
    }
}
