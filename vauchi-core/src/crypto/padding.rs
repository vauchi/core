// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fixed-Size Message Padding
//!
//! Pads messages to fixed bucket sizes to prevent traffic analysis. Without
//! padding, an observer can distinguish message types by size (e.g., ~150B
//! for presence, ~500B for card deltas, ~2KB+ for media).
//!
//! ## Bucket Sizes
//!
//! | Bucket | Size  | Typical Content |
//! |--------|-------|-----------------|
//! | Small  | 256 B | ACK, presence, revocation |
//! | Medium | 1 KB  | Card deltas, small updates |
//! | Large  | 4 KB  | Media references, large payloads |
//!
//! Messages larger than the large bucket are rounded up to the next
//! 256-byte boundary.
//!
//! ## Wire Format
//!
//! ```text
//! [original length: 4 bytes BE] [plaintext] [random padding bytes]
//! ```
//!
//! The first 4 bytes are the big-endian original plaintext length.
//! This allows the receiver to strip padding after decryption.

use ring::rand::{SecureRandom, SystemRandom};

/// Bucket sizes in bytes (including the 4-byte length prefix).
const BUCKET_SMALL: usize = 256;
const BUCKET_MEDIUM: usize = 1024;
const BUCKET_LARGE: usize = 4096;

/// Alignment for oversized messages.
const OVERFLOW_ALIGNMENT: usize = 256;

/// Size of the length prefix (4 bytes, big-endian).
const LENGTH_PREFIX_SIZE: usize = 4;

/// Pads plaintext to the nearest bucket size.
///
/// Returns a buffer of exactly the bucket size, containing a 4-byte
/// big-endian length prefix, the original plaintext, and random padding.
pub fn pad(plaintext: &[u8]) -> Vec<u8> {
    let needed = LENGTH_PREFIX_SIZE + plaintext.len();
    let target_size = select_bucket(needed);

    let mut padded = Vec::with_capacity(target_size);

    // Write original length as 4-byte big-endian prefix
    padded.extend_from_slice(&(plaintext.len() as u32).to_be_bytes());

    // Copy plaintext
    padded.extend_from_slice(plaintext);

    // Fill remaining space with random bytes
    let padding_len = target_size - needed;
    if padding_len > 0 {
        padded.resize(target_size, 0);
        let rng = SystemRandom::new();
        rng.fill(&mut padded[needed..])
            .expect("System RNG should not fail");
    }

    padded
}

/// Removes padding and returns the original plaintext.
///
/// Reads the 4-byte length prefix, then extracts that many bytes
/// of plaintext immediately following the prefix.
pub fn unpad(padded: &[u8]) -> Option<Vec<u8>> {
    if padded.len() < LENGTH_PREFIX_SIZE {
        return None;
    }

    // Read original length from first 4 bytes
    let len = u32::from_be_bytes([padded[0], padded[1], padded[2], padded[3]]) as usize;

    // Validate: plaintext must fit within the padded buffer
    if LENGTH_PREFIX_SIZE + len > padded.len() {
        return None;
    }

    Some(padded[LENGTH_PREFIX_SIZE..LENGTH_PREFIX_SIZE + len].to_vec())
}

/// Validates that a received padded buffer has a valid bucket size (#149).
///
/// Returns `true` if the buffer length matches a known bucket (256, 1024, 4096)
/// or is aligned to `OVERFLOW_ALIGNMENT` for oversized messages.
/// An unexpected size may indicate tampering or a protocol mismatch.
pub fn is_valid_bucket_size(len: usize) -> bool {
    len == BUCKET_SMALL
        || len == BUCKET_MEDIUM
        || len == BUCKET_LARGE
        || (len > BUCKET_LARGE && len % OVERFLOW_ALIGNMENT == 0)
}

/// Selects the smallest bucket that fits the given size.
fn select_bucket(size: usize) -> usize {
    if size <= BUCKET_SMALL {
        BUCKET_SMALL
    } else if size <= BUCKET_MEDIUM {
        BUCKET_MEDIUM
    } else if size <= BUCKET_LARGE {
        BUCKET_LARGE
    } else {
        // Round up to next OVERFLOW_ALIGNMENT boundary
        size.div_ceil(OVERFLOW_ALIGNMENT) * OVERFLOW_ALIGNMENT
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_unpad_roundtrip_small() {
        let plaintext = b"hello";
        let padded = pad(plaintext);
        assert_eq!(padded.len(), BUCKET_SMALL);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_unpad_roundtrip_medium() {
        let plaintext = vec![0xAB; 300]; // Exceeds small bucket
        let padded = pad(&plaintext);
        assert_eq!(padded.len(), BUCKET_MEDIUM);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_unpad_roundtrip_large() {
        let plaintext = vec![0xCD; 2000]; // Exceeds medium bucket
        let padded = pad(&plaintext);
        assert_eq!(padded.len(), BUCKET_LARGE);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_overflow_alignment() {
        let plaintext = vec![0xEF; 5000]; // Exceeds large bucket
        let padded = pad(&plaintext);
        assert_eq!(padded.len() % OVERFLOW_ALIGNMENT, 0);
        assert!(padded.len() >= 5004); // plaintext + 4 byte length
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_very_large_message() {
        let plaintext = vec![0xDE; 100_000]; // 100KB message
        let padded = pad(&plaintext);
        assert_eq!(padded.len() % OVERFLOW_ALIGNMENT, 0);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_empty_plaintext() {
        let padded = pad(b"");
        assert_eq!(padded.len(), BUCKET_SMALL);
        let recovered = unpad(&padded).unwrap();
        assert!(recovered.is_empty());
    }

    #[test]
    fn test_pad_exact_bucket_boundary() {
        // Plaintext that exactly fills small bucket (256 - 4 = 252 bytes)
        let plaintext = vec![0x42; BUCKET_SMALL - LENGTH_PREFIX_SIZE];
        let padded = pad(&plaintext);
        assert_eq!(padded.len(), BUCKET_SMALL);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_pad_one_byte_over_bucket() {
        // One byte over small bucket boundary -> medium
        let plaintext = vec![0x42; BUCKET_SMALL - LENGTH_PREFIX_SIZE + 1];
        let padded = pad(&plaintext);
        assert_eq!(padded.len(), BUCKET_MEDIUM);
        let recovered = unpad(&padded).unwrap();
        assert_eq!(recovered, plaintext);
    }

    #[test]
    fn test_unpad_invalid_too_short() {
        assert!(unpad(&[]).is_none());
        assert!(unpad(&[0x01]).is_none());
        assert!(unpad(&[0x00, 0x00, 0x00]).is_none());
    }

    #[test]
    fn test_unpad_invalid_length_exceeds_buffer() {
        // Length prefix claims 255 bytes but buffer is only 8 bytes
        let bad = [0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0x00];
        assert!(unpad(&bad).is_none());
    }

    #[test]
    fn test_all_bucket_sizes_are_consistent() {
        // Test various sizes hit expected buckets
        for size in 0..=252 {
            let plaintext = vec![0u8; size];
            assert_eq!(pad(&plaintext).len(), BUCKET_SMALL, "size={}", size);
        }
        for size in 253..=1020 {
            let plaintext = vec![0u8; size];
            assert_eq!(pad(&plaintext).len(), BUCKET_MEDIUM, "size={}", size);
        }
        for size in 1021..=4092 {
            let plaintext = vec![0u8; size];
            assert_eq!(pad(&plaintext).len(), BUCKET_LARGE, "size={}", size);
        }
    }

    #[test]
    fn test_padded_output_differs_between_calls() {
        // Random padding means same plaintext produces different padded output
        let plaintext = b"test";
        let padded1 = pad(plaintext);
        let padded2 = pad(plaintext);
        // Both should unpad to same value
        assert_eq!(unpad(&padded1).unwrap(), plaintext);
        assert_eq!(unpad(&padded2).unwrap(), plaintext);
        // The random tail portion should differ
        // (first 4+4=8 bytes are deterministic: length + plaintext)
        assert_ne!(padded1[8..], padded2[8..]);
    }

    #[test]
    fn test_is_valid_bucket_size() {
        assert!(is_valid_bucket_size(BUCKET_SMALL));
        assert!(is_valid_bucket_size(BUCKET_MEDIUM));
        assert!(is_valid_bucket_size(BUCKET_LARGE));
        assert!(is_valid_bucket_size(4352)); // Overflow aligned
        assert!(is_valid_bucket_size(4608));
        assert!(!is_valid_bucket_size(0));
        assert!(!is_valid_bucket_size(100)); // Not a valid bucket
        assert!(!is_valid_bucket_size(512)); // Between small and medium
        assert!(!is_valid_bucket_size(4097)); // Not aligned
    }

    #[test]
    fn test_select_bucket() {
        assert_eq!(select_bucket(1), BUCKET_SMALL);
        assert_eq!(select_bucket(256), BUCKET_SMALL);
        assert_eq!(select_bucket(257), BUCKET_MEDIUM);
        assert_eq!(select_bucket(1024), BUCKET_MEDIUM);
        assert_eq!(select_bucket(1025), BUCKET_LARGE);
        assert_eq!(select_bucket(4096), BUCKET_LARGE);
        assert_eq!(select_bucket(4097), 4352); // Next 256-byte boundary
    }
}
