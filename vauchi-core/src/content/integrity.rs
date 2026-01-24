//! Content integrity verification using SHA-256 checksums
//!
//! All remote content is verified using SHA-256 checksums before being
//! saved to the local cache. This ensures content has not been tampered
//! with during transit.

use ring::digest::{Context, SHA256};
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
}

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
}
