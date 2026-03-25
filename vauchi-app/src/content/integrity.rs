// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Content integrity verification using SHA-256 checksums
//!
//! All remote content is verified using SHA-256 checksums before being
//! saved to the local cache. This ensures content has not been tampered
//! with during transit.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
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
/// use vauchi_app::content::{verify_checksum, compute_checksum};
///
/// let data = b"hello world";
/// let checksum = compute_checksum(data);
/// verify_checksum(data, &checksum).expect("valid checksum should verify");
/// ```
pub fn verify_checksum(data: &[u8], expected: &str) -> Result<(), IntegrityError> {
    // Expected format: "sha256:hexstring"
    let expected_hex = expected
        .strip_prefix("sha256:")
        .ok_or(IntegrityError::InvalidFormat)?;

    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    let actual_hex = hex::encode(&hash[..]);

    // SHA-256 hex is always 64 chars; reject malformed input before ct_eq
    // (ct_eq on different-length slices short-circuits, leaking length)
    if expected_hex.len() != 64 {
        return Err(IntegrityError::InvalidFormat);
    }

    if bool::from(actual_hex.as_bytes().ct_eq(expected_hex.as_bytes())) {
        Ok(())
    } else {
        Err(IntegrityError::ChecksumMismatch)
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
/// use vauchi_app::content::compute_checksum;
///
/// let data = b"hello world";
/// let checksum = compute_checksum(data);
/// assert!(checksum.starts_with("sha256:"));
/// ```
pub fn compute_checksum(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    format!("sha256:{}", hex::encode(&hash[..]))
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
    public_key: &vauchi_core::crypto::signing::PublicKey,
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

    let signature = vauchi_core::crypto::signing::Signature::from_bytes(
        sig_bytes
            .try_into()
            .map_err(|_| IntegrityError::InvalidSignatureFormat)?,
    );

    // Compute canonical signed data: manifest JSON without the signature field.
    // Serialize via serde_json::Value to get sorted keys (BTreeMap-backed Map),
    // ensuring deterministic output regardless of HashMap iteration order.
    // This matches Python's json.dumps(sort_keys=True, separators=(",",":")).
    let mut manifest_for_signing = manifest.clone();
    manifest_for_signing.signature = None;
    let value = serde_json::to_value(&manifest_for_signing)
        .map_err(|e| IntegrityError::SignatureVerificationFailed(e.to_string()))?;
    let canonical_json = serde_json::to_vec(&value)
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
    #[error("Checksum mismatch")]
    ChecksumMismatch,

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
        assert!(verify_checksum(data, expected).is_ok(), "expected success");
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_valid() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use vauchi_core::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        // Create manifest without signature
        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        // Sign: canonical JSON of manifest without signature (via Value for sorted keys)
        let value = serde_json::to_value(&manifest).unwrap();
        let canonical_json = serde_json::to_vec(&value).unwrap();
        let signature = keypair.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Verify
        assert!(
            verify_manifest_signature(&manifest, &public_key).is_ok(),
            "expected success"
        );
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_tampered() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use vauchi_core::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        // Sign the original (via Value for sorted keys)
        let value = serde_json::to_value(&manifest).unwrap();
        let canonical_json = serde_json::to_vec(&value).unwrap();
        let signature = keypair.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Tamper with the manifest
        manifest.base_url = "https://evil.example.com/files".to_string();

        // Verification should fail
        assert!(
            verify_manifest_signature(&manifest, &public_key).is_err(),
            "expected error"
        );
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_missing() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use vauchi_core::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        let manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        let result = verify_manifest_signature(&manifest, &public_key);
        assert!(result.is_err(), "expected error");
        assert!(matches!(
            result.unwrap_err(),
            IntegrityError::MissingSignature
        ));
    }

    // Trace: codebase-review-tracker item #24
    #[test]
    fn test_verify_manifest_signature_wrong_key() {
        use crate::content::types::{ContentIndex, ContentManifest};
        use vauchi_core::crypto::signing::SigningKeyPair;

        let signer = SigningKeyPair::generate();
        let wrong_key = SigningKeyPair::generate().public_key();

        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-02-14T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        let canonical_json = serde_json::to_vec(&manifest).unwrap();
        let signature = signer.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Verification with wrong key should fail
        assert!(
            verify_manifest_signature(&manifest, &wrong_key).is_err(),
            "expected error"
        );
    }

    /// Verify that signing works with locale files (HashMap determinism).
    ///
    /// Before the sorted-key fix, HashMap iteration order made canonical
    /// JSON non-deterministic for manifests with multiple locale files.
    #[test]
    fn test_verify_manifest_signature_with_locales() {
        use crate::content::types::{ContentIndex, ContentManifest, FileEntry, LocalesEntry};
        use std::collections::HashMap;
        use vauchi_core::crypto::signing::SigningKeyPair;

        let keypair = SigningKeyPair::generate();
        let public_key = keypair.public_key();

        let mut files = HashMap::new();
        files.insert(
            "en".to_string(),
            FileEntry {
                path: "en.json".to_string(),
                checksum: "sha256:aaa".to_string(),
                size_bytes: 1000,
            },
        );
        files.insert(
            "de".to_string(),
            FileEntry {
                path: "de.json".to_string(),
                checksum: "sha256:bbb".to_string(),
                size_bytes: 1100,
            },
        );
        files.insert(
            "fr".to_string(),
            FileEntry {
                path: "fr.json".to_string(),
                checksum: "sha256:ccc".to_string(),
                size_bytes: 1200,
            },
        );

        let mut manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-03-18T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1/".to_string(),
            content: ContentIndex {
                locales: Some(LocalesEntry {
                    version: "1.0.0".to_string(),
                    path: "locales/".to_string(),
                    files,
                    min_app_version: "0.1.0".to_string(),
                }),
                ..ContentIndex::default()
            },
            signature: None,
        };

        // Sign
        let value = serde_json::to_value(&manifest).unwrap();
        let canonical_json = serde_json::to_vec(&value).unwrap();
        let signature = keypair.sign(&canonical_json);
        manifest.signature = Some(hex::encode(signature.as_bytes()));

        // Verify multiple times to catch HashMap ordering flakiness
        for _ in 0..10 {
            assert!(
                verify_manifest_signature(&manifest, &public_key).is_ok(),
                "Signature verification should be deterministic with locale files"
            );
        }
    }

    /// Verify canonical JSON matches Python's json.dumps(sort_keys=True).
    ///
    /// The CI signing script (sign-manifest.py) uses:
    ///   json.dumps(data, sort_keys=True, separators=(",",":"))
    /// Core must produce identical bytes for signature verification.
    #[test]
    fn test_canonical_json_matches_python_sorted_keys() {
        use crate::content::types::{ContentIndex, ContentManifest};

        let manifest = ContentManifest {
            schema_version: 1,
            generated_at: "2026-01-01T00:00:00Z".to_string(),
            base_url: "https://cdn.vauchi.app/v1/".to_string(),
            content: ContentIndex::default(),
            signature: None,
        };

        let value = serde_json::to_value(&manifest).unwrap();
        let canonical = serde_json::to_vec(&value).unwrap();
        let canonical_str = String::from_utf8(canonical).unwrap();

        // Must match Python: json.dumps(sort_keys=True, separators=(",",":"))
        // Keys sorted alphabetically: base_url, content, generated_at, schema_version
        assert!(
            canonical_str.starts_with(r#"{"base_url":"#),
            "Canonical JSON must have sorted keys (base_url first), got: {}",
            &canonical_str[..50.min(canonical_str.len())]
        );
        assert!(
            !canonical_str.contains(' '),
            "Canonical JSON must be compact (no spaces)"
        );
        // Signature field must not be present (skip_serializing_if = None)
        assert!(
            !canonical_str.contains("signature"),
            "Canonical JSON must not contain signature field when None"
        );
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

    /// Contract test: validates core can deserialize the manifest format
    /// produced by website/scripts/build-manifest.py.
    ///
    /// If this test fails, build-manifest.py output has drifted from core's
    /// ContentManifest struct. Fix by updating build-manifest.py to include
    /// the new/changed fields.
    ///
    /// Set VAUCHI_MANIFEST_PATH to a real manifest to test against production.
    /// Without it, the test uses an inline sample matching build-manifest.py output.
    #[test]
    fn test_deserialize_build_manifest_output() {
        use crate::content::types::ContentManifest;

        let json = match std::env::var("VAUCHI_MANIFEST_PATH") {
            Ok(path) => std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("Failed to read manifest at {path}: {e}")),
            Err(_) => {
                // Inline sample matching build-manifest.py output format exactly.
                // Keep this in sync when adding fields to ContentManifest/ContentEntry/etc.
                r#"{
                    "schema_version": 1,
                    "generated_at": "2026-03-18T00:00:00+00:00",
                    "base_url": "https://cdn.vauchi.app/v1/",
                    "content": {
                        "networks": {
                            "version": "1.0.0",
                            "path": "networks.json",
                            "checksum": "sha256:aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
                            "size_bytes": 3211,
                            "min_app_version": "0.1.0"
                        },
                        "locales": {
                            "version": "1.0.0",
                            "path": "locales/",
                            "min_app_version": "0.1.0",
                            "files": {
                                "en": {
                                    "path": "en.json",
                                    "checksum": "sha256:aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
                                    "size_bytes": 56311
                                },
                                "de": {
                                    "path": "de.json",
                                    "checksum": "sha256:aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
                                    "size_bytes": 62389
                                }
                            }
                        },
                        "themes": {
                            "version": "1.0.0",
                            "path": "themes/themes.json",
                            "checksum": "sha256:aabbccdd00112233445566778899aabbccddeeff00112233445566778899aabb",
                            "size_bytes": 7743,
                            "min_app_version": "0.1.0"
                        }
                    },
                    "signature": "aabbccdd00112233"
                }"#
                .to_string()
            }
        };

        let manifest: ContentManifest = serde_json::from_str(&json).expect(
            "Core cannot deserialize build-manifest.py output — struct/script drift detected",
        );

        assert_eq!(manifest.schema_version, 1);
        assert!(!manifest.base_url.is_empty());

        // Verify content entries deserialized with all required fields
        if let Some(ref networks) = manifest.content.networks {
            assert!(!networks.version.is_empty());
            assert!(networks.size_bytes > 0, "networks must have size_bytes");
        }
        if let Some(ref locales) = manifest.content.locales {
            assert!(
                !locales.files.is_empty(),
                "locales must have at least one file"
            );
            for (lang, entry) in &locales.files {
                assert!(!lang.is_empty());
                assert!(entry.size_bytes > 0, "locale {lang} must have size_bytes");
            }
        }
        if let Some(ref themes) = manifest.content.themes {
            assert!(themes.size_bytes > 0, "themes must have size_bytes");
        }

        println!(
            "Contract check passed: manifest deserialized with {} content types",
            [
                manifest.content.networks.is_some(),
                manifest.content.locales.is_some(),
                manifest.content.themes.is_some(),
            ]
            .iter()
            .filter(|x| **x)
            .count()
        );
    }
}
