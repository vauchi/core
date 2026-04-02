// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multipart QR Codec
//!
//! Encodes large payloads into multiple QR-sized chunks and reassembles them.
//! Enables offline device linking via animated QR code sequences.
//!
//! Chunk format: `{index}/{total}/{crc32_hex_8chars}/{base64url_data}`

use std::collections::HashMap;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use thiserror::Error;

/// Errors from multipart QR encoding/decoding.
#[derive(Error, Debug)]
pub enum MultipartQrError {
    #[error("invalid chunk format: expected index/total/crc32/data")]
    InvalidFormat,

    #[error("invalid chunk index: {0}")]
    InvalidIndex(#[source] std::num::ParseIntError),

    #[error("invalid chunk total: {0}")]
    InvalidTotal(#[source] std::num::ParseIntError),

    #[error("invalid chunk total: must be > 0")]
    ZeroTotal,

    #[error("chunk index {index} out of range for total {total}")]
    IndexOutOfRange { index: usize, total: usize },

    #[error("invalid CRC32 hex length: expected 8, got {len}")]
    InvalidCrcLength { len: usize },

    #[error("chunk total mismatch: expected {expected}, got {got}")]
    TotalMismatch { expected: usize, got: usize },

    #[error("invalid base64url data: {0}")]
    InvalidBase64(#[from] base64::DecodeError),

    #[error("CRC32 checksum mismatch for chunk {index}: expected {expected:08x}, got {actual:08x}")]
    CrcMismatch {
        index: usize,
        expected: u32,
        actual: u32,
    },

    #[error("invalid CRC32 hex: {0}")]
    InvalidCrcHex(#[source] std::num::ParseIntError),

    #[error("no chunks received yet")]
    NoChunks,

    #[error("incomplete: received {received}/{total} chunks")]
    Incomplete { received: usize, total: usize },

    #[error("missing chunk {index}")]
    MissingChunk { index: usize },
}

/// Encode a byte payload into multiple QR-sized chunk strings.
///
/// Each chunk has the format: `{index}/{total}/{crc32_hex_8chars}/{base64url_data}`
///
/// - `data`: the raw bytes to encode
/// - `max_chunk_bytes`: maximum byte length of each chunk string
///
/// Returns a `Vec<String>` of chunks ordered by index (0-based).
pub fn encode_multipart(data: &[u8], max_chunk_bytes: usize) -> Vec<String> {
    // We need to figure out how many chunks we need. The overhead per chunk is:
    //   len(index_str) + 1 + len(total_str) + 1 + 8 + 1 = len(index_str) + len(total_str) + 11
    //
    // Base64url-no-pad encoding: ceil(n * 4 / 3) chars for n raw bytes.
    // We iterate to find the right split because the number of digits in the total
    // changes the overhead.

    if data.is_empty() {
        // Special case: empty payload produces one chunk with empty data
        let crc = crc32fast::hash(b"");
        return vec![format!("0/1/{crc:08x}/")];
    }

    // Estimate the number of chunks needed, then refine
    let total = compute_chunk_count(data.len(), max_chunk_bytes);

    let total_str_len = digit_count(total);

    // Compute max raw bytes per chunk such that the encoded chunk fits in max_chunk_bytes
    let mut chunks = Vec::with_capacity(total);
    let mut offset = 0;

    for i in 0..total {
        let index_str_len = digit_count(i);
        // overhead = index_digits + '/' + total_digits + '/' + 8 (crc hex) + '/'
        let overhead = index_str_len + 1 + total_str_len + 1 + 8 + 1;
        let max_b64_len = max_chunk_bytes.saturating_sub(overhead);
        // base64url-no-pad: 4 output chars per 3 input bytes
        // max_raw = floor(max_b64_len * 3 / 4)
        let max_raw = max_b64_len * 3 / 4;

        let end = (offset + max_raw).min(data.len());
        let chunk_data = &data[offset..end];
        offset = end;

        let b64 = URL_SAFE_NO_PAD.encode(chunk_data);
        let crc = crc32fast::hash(chunk_data);
        chunks.push(format!("{i}/{total}/{crc:08x}/{b64}"));
    }

    chunks
}

/// Compute the number of chunks needed for `data_len` bytes with `max_chunk_bytes` limit.
fn compute_chunk_count(data_len: usize, max_chunk_bytes: usize) -> usize {
    // Start with a guess and refine
    for candidate_total in 1..=data_len + 1 {
        let total_str_len = digit_count(candidate_total);
        // Worst-case overhead is for the last chunk (largest index)
        let max_index_str_len = digit_count(candidate_total - 1);
        let overhead = max_index_str_len + 1 + total_str_len + 1 + 8 + 1;
        let max_b64_len = max_chunk_bytes.saturating_sub(overhead);
        let max_raw_per_chunk = max_b64_len * 3 / 4;

        if max_raw_per_chunk == 0 {
            continue;
        }

        // Total raw bytes this many chunks can carry
        // For a more precise calculation, sum across all chunks (some have smaller index digits)
        // But worst-case estimate is good enough and simpler
        let total_capacity = max_raw_per_chunk * candidate_total;
        if total_capacity >= data_len {
            return candidate_total;
        }
    }

    // Fallback: one byte per chunk (should never reach here for reasonable inputs)
    data_len
}

/// Count the number of decimal digits in a number.
fn digit_count(n: usize) -> usize {
    if n == 0 {
        return 1;
    }
    ((n as f64).log10().floor() as usize) + 1
}

/// Decoder for reassembling multipart QR chunks.
///
/// Chunks can be added in any order. Duplicates are detected and ignored.
pub struct MultipartDecoder {
    /// Expected total number of chunks (set from first chunk received)
    total: Option<usize>,
    /// Received chunks indexed by their position
    chunks: HashMap<usize, Vec<u8>>,
}

impl Default for MultipartDecoder {
    fn default() -> Self {
        Self::new()
    }
}

impl MultipartDecoder {
    /// Create a new empty decoder.
    pub fn new() -> Self {
        Self {
            total: None,
            chunks: HashMap::new(),
        }
    }

    /// Add a chunk string to the decoder.
    ///
    /// Returns `Ok(true)` if the chunk is new, `Ok(false)` if it was a duplicate.
    /// Returns `Err` if the chunk format is invalid or the CRC checksum fails.
    pub fn add_chunk(&mut self, raw: &str) -> Result<bool, MultipartQrError> {
        let parts: Vec<&str> = raw.splitn(4, '/').collect();
        if parts.len() < 4 {
            return Err(MultipartQrError::InvalidFormat);
        }

        let index: usize = parts[0].parse().map_err(MultipartQrError::InvalidIndex)?;
        let total: usize = parts[1].parse().map_err(MultipartQrError::InvalidTotal)?;
        let expected_crc_hex = parts[2];
        let b64_data = parts[3];

        if total == 0 {
            return Err(MultipartQrError::ZeroTotal);
        }
        if index >= total {
            return Err(MultipartQrError::IndexOutOfRange { index, total });
        }
        if expected_crc_hex.len() != 8 {
            return Err(MultipartQrError::InvalidCrcLength {
                len: expected_crc_hex.len(),
            });
        }

        // Check total consistency
        if let Some(existing_total) = self.total
            && total != existing_total
        {
            return Err(MultipartQrError::TotalMismatch {
                expected: existing_total,
                got: total,
            });
        }

        // Decode the base64 data
        let chunk_bytes = URL_SAFE_NO_PAD.decode(b64_data)?;

        // Verify CRC32
        let actual_crc = crc32fast::hash(&chunk_bytes);
        let expected_crc =
            u32::from_str_radix(expected_crc_hex, 16).map_err(MultipartQrError::InvalidCrcHex)?;

        if actual_crc != expected_crc {
            return Err(MultipartQrError::CrcMismatch {
                index,
                expected: expected_crc,
                actual: actual_crc,
            });
        }

        // Store total on first chunk
        self.total = Some(total);

        // Check for duplicate
        if self.chunks.contains_key(&index) {
            return Ok(false);
        }

        self.chunks.insert(index, chunk_bytes);
        Ok(true)
    }

    /// Number of unique chunks received so far.
    pub fn received(&self) -> usize {
        self.chunks.len()
    }

    /// Expected total number of chunks, if at least one has been received.
    pub fn expected_total(&self) -> Option<usize> {
        self.total
    }

    /// Whether all chunks have been received.
    pub fn is_complete(&self) -> bool {
        match self.total {
            Some(total) => self.chunks.len() == total,
            None => false,
        }
    }

    /// Reassemble the original payload from received chunks.
    ///
    /// Returns `Err` if not all chunks have been received yet.
    pub fn assemble(&self) -> Result<Vec<u8>, MultipartQrError> {
        let total = self.total.ok_or(MultipartQrError::NoChunks)?;

        if self.chunks.len() != total {
            return Err(MultipartQrError::Incomplete {
                received: self.chunks.len(),
                total,
            });
        }

        let mut result = Vec::new();
        for i in 0..total {
            let chunk = self
                .chunks
                .get(&i)
                .ok_or(MultipartQrError::MissingChunk { index: i })?;
            result.extend_from_slice(chunk);
        }

        Ok(result)
    }
}

// === UniFFI Wrapper ===

use crate::error::{MobileError, lock_or};

/// UniFFI-friendly wrapper around `MultipartDecoder`.
///
/// Exposes the decoder as an Arc-wrapped object that mobile platforms (iOS/Android)
/// can use to reassemble animated QR code sequences. The inner `MultipartDecoder`
/// is protected by a `Mutex` for thread safety.
#[derive(uniffi::Object)]
pub struct MobileMultipartDecoder {
    inner: std::sync::Mutex<MultipartDecoder>,
}

impl Default for MobileMultipartDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[uniffi::export]
impl MobileMultipartDecoder {
    /// Create a new empty multipart QR decoder.
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(MultipartDecoder::new()),
        }
    }

    /// Add a scanned QR chunk string. Returns `true` if new, `false` if duplicate.
    pub fn add_chunk(&self, chunk: String) -> Result<bool, MobileError> {
        lock_or(&self.inner)?
            .add_chunk(&chunk)
            .map_err(|e| MobileError::InvalidInput(e.to_string()))
    }

    /// Number of unique chunks received so far.
    pub fn received(&self) -> u32 {
        let Ok(guard) = self.inner.lock() else {
            return 0;
        };
        guard.received() as u32
    }

    /// Expected total number of chunks, if at least one has been received.
    pub fn expected_total(&self) -> Option<u32> {
        let Ok(guard) = self.inner.lock() else {
            return None;
        };
        guard.expected_total().map(|t| t as u32)
    }

    /// Whether all chunks have been received.
    pub fn is_complete(&self) -> bool {
        let Ok(guard) = self.inner.lock() else {
            return false;
        };
        guard.is_complete()
    }

    /// Reassemble the complete payload from received chunks.
    ///
    /// Only valid when `is_complete()` returns `true`.
    pub fn assemble(&self) -> Result<Vec<u8>, MobileError> {
        lock_or(&self.inner)?
            .assemble()
            .map_err(|e| MobileError::InvalidInput(e.to_string()))
    }
}

// INLINE_TEST_REQUIRED: Tests exercise internal chunk parsing and CRC verification logic

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // encode_multipart tests
    // =========================================================================

    #[test]
    fn test_encode_single_chunk_small_payload_fits_in_one() {
        let data = b"hello";
        let chunks = encode_multipart(data, 256);

        assert_eq!(
            chunks.len(),
            1,
            "small payload should produce exactly 1 chunk"
        );

        // Parse the chunk format: index/total/crc32/data
        let parts: Vec<&str> = chunks[0].splitn(4, '/').collect();
        assert_eq!(parts.len(), 4, "chunk must have 4 slash-separated parts");
        assert_eq!(parts[0], "0", "first chunk index must be 0");
        assert_eq!(parts[1], "1", "total must be 1 for single chunk");
        assert_eq!(parts[2].len(), 8, "CRC32 hex must be 8 chars");

        // Verify the data decodes back
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        let decoded = URL_SAFE_NO_PAD.decode(parts[3]).expect("valid base64url");
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_encode_multiple_chunks_large_payload_splits() {
        // 5KB payload should split into 3+ chunks with 2048-byte max
        let data = vec![0xAB; 5120]; // 5KB
        let chunks = encode_multipart(&data, 2048);

        assert!(
            chunks.len() >= 3,
            "5KB payload with 2048-byte max should produce at least 3 chunks, got {}",
            chunks.len()
        );

        // Every chunk should have consistent total
        for (i, chunk) in chunks.iter().enumerate() {
            let parts: Vec<&str> = chunk.splitn(4, '/').collect();
            assert_eq!(parts.len(), 4, "chunk {i} must have 4 parts");
            let index: usize = parts[0].parse().expect("valid index");
            let total: usize = parts[1].parse().expect("valid total");
            assert_eq!(index, i, "chunk index must match position");
            assert_eq!(
                total,
                chunks.len(),
                "total must be consistent across chunks"
            );
            assert_eq!(parts[2].len(), 8, "CRC32 hex must be 8 chars");
        }
    }

    // =========================================================================
    // MultipartDecoder tests
    // =========================================================================

    #[test]
    fn test_roundtrip_encode_then_decode_in_order() {
        let original = b"The quick brown fox jumps over the lazy dog. \
                         This is a longer message for roundtrip testing.";
        let chunks = encode_multipart(original, 64);
        assert!(
            chunks.len() > 1,
            "should produce multiple chunks for roundtrip test"
        );

        let mut decoder = MultipartDecoder::new();
        for chunk in &chunks {
            let is_new = decoder.add_chunk(chunk).expect("valid chunk");
            assert!(is_new, "first add of each chunk should return true");
        }

        assert!(
            decoder.is_complete(),
            "all chunks added, should be complete"
        );
        assert_eq!(decoder.received(), chunks.len());
        assert_eq!(decoder.expected_total(), Some(chunks.len()));

        let assembled = decoder.assemble().expect("assemble should succeed");
        assert_eq!(assembled, original, "reassembled data must match original");
    }

    #[test]
    fn test_roundtrip_out_of_order_decode_chunks_reversed() {
        let original = vec![42u8; 1000];
        let chunks = encode_multipart(&original, 128);
        assert!(chunks.len() > 1, "should produce multiple chunks");

        let mut decoder = MultipartDecoder::new();
        // Feed chunks in reverse order
        for chunk in chunks.iter().rev() {
            let is_new = decoder.add_chunk(chunk).expect("valid chunk");
            assert!(is_new, "first add of each chunk should return true");
        }

        assert!(
            decoder.is_complete(),
            "all chunks added (reverse), should be complete"
        );
        let assembled = decoder.assemble().expect("assemble should succeed");
        assert_eq!(
            assembled, original,
            "reassembled data must match original regardless of order"
        );
    }

    #[test]
    fn test_duplicate_chunks_ignored_returns_false() {
        let data = b"duplicate test data with enough length to split";
        let chunks = encode_multipart(data, 32);
        assert!(chunks.len() > 1, "need multiple chunks");

        let mut decoder = MultipartDecoder::new();

        // Add first chunk - should be new
        let first_add = decoder.add_chunk(&chunks[0]).expect("valid chunk");
        assert!(first_add, "first add should return Ok(true)");

        // Add same chunk again - should be duplicate
        let second_add = decoder.add_chunk(&chunks[0]).expect("valid chunk");
        assert!(!second_add, "duplicate add should return Ok(false)");

        // Count should still be 1
        assert_eq!(
            decoder.received(),
            1,
            "duplicate should not increment count"
        );
    }

    #[test]
    fn test_checksum_mismatch_rejected_corrupted_data() {
        let data = b"checksum test data";
        let chunks = encode_multipart(data, 256);
        assert!(!chunks.is_empty());

        // Corrupt the chunk: change data portion but keep same checksum
        let parts: Vec<&str> = chunks[0].splitn(4, '/').collect();
        let corrupted = format!("{}/{}/{}/AAAA", parts[0], parts[1], parts[2]);

        let mut decoder = MultipartDecoder::new();
        let result = decoder.add_chunk(&corrupted);
        assert!(result.is_err(), "corrupted chunk should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("checksum") || err_msg.contains("CRC"),
            "error should mention checksum, got: {err_msg}"
        );
    }

    #[test]
    fn test_assemble_incomplete_returns_error() {
        let data = vec![0u8; 500];
        let chunks = encode_multipart(&data, 64);
        assert!(chunks.len() > 2, "need several chunks");

        let mut decoder = MultipartDecoder::new();
        decoder.add_chunk(&chunks[0]).expect("valid chunk");

        assert!(
            !decoder.is_complete(),
            "should not be complete with only 1 chunk"
        );
        let result = decoder.assemble();
        assert!(result.is_err(), "assembling incomplete data should error");
    }

    #[test]
    fn test_decoder_new_starts_empty() {
        let decoder = MultipartDecoder::new();
        assert_eq!(decoder.received(), 0);
        assert_eq!(decoder.expected_total(), None);
        assert!(!decoder.is_complete());
    }

    #[test]
    fn test_encode_empty_payload_produces_one_chunk() {
        let data = b"";
        let chunks = encode_multipart(data, 256);
        assert_eq!(
            chunks.len(),
            1,
            "empty payload should still produce 1 chunk"
        );

        let mut decoder = MultipartDecoder::new();
        decoder.add_chunk(&chunks[0]).expect("valid chunk");
        assert!(decoder.is_complete());
        let assembled = decoder.assemble().expect("assemble should succeed");
        assert!(
            assembled.is_empty(),
            "assembled empty payload should be empty"
        );
    }

    #[test]
    fn test_invalid_chunk_format_rejected() {
        let mut decoder = MultipartDecoder::new();

        // Missing parts
        let result = decoder.add_chunk("not-a-valid-chunk");
        assert!(result.is_err(), "invalid format should be rejected");

        // Invalid index
        let result = decoder.add_chunk("abc/3/00000000/AAAA");
        assert!(result.is_err(), "non-numeric index should be rejected");

        // Invalid total
        let result = decoder.add_chunk("0/abc/00000000/AAAA");
        assert!(result.is_err(), "non-numeric total should be rejected");
    }

    #[test]
    fn test_chunk_crc32_is_verified() {
        // Manually construct a chunk with correct format but wrong CRC
        use base64::Engine;
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;

        let payload = b"test payload";
        let encoded = URL_SAFE_NO_PAD.encode(payload);
        // Use a definitely wrong CRC
        let bad_chunk = format!("0/1/DEADBEEF/{encoded}");

        let mut decoder = MultipartDecoder::new();
        let result = decoder.add_chunk(&bad_chunk);
        assert!(result.is_err(), "wrong CRC should be rejected");
    }

    // =========================================================================
    // Proptest
    // =========================================================================

    mod proptests {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(200))]

            #[test]
            fn test_roundtrip_arbitrary(
                data in prop::collection::vec(any::<u8>(), 1..20000),
                max_chunk in 64usize..4096
            ) {
                let chunks = encode_multipart(&data, max_chunk);

                // All chunks should be <= max_chunk bytes
                for (i, chunk) in chunks.iter().enumerate() {
                    prop_assert!(
                        chunk.len() <= max_chunk,
                        "chunk {} length {} exceeds max {}",
                        i, chunk.len(), max_chunk
                    );
                }

                // Roundtrip must succeed
                let mut decoder = MultipartDecoder::new();
                for chunk in &chunks {
                    decoder.add_chunk(chunk).expect("valid chunk");
                }

                prop_assert!(decoder.is_complete(), "decoder should be complete");
                let assembled = decoder.assemble().expect("assemble should succeed");
                prop_assert_eq!(assembled, data, "roundtrip must preserve data");
            }
        }
    }

    // =========================================================================
    // MobileMultipartDecoder (UniFFI wrapper) tests
    // =========================================================================

    #[test]
    fn test_mobile_decoder_new_starts_empty() {
        let decoder = MobileMultipartDecoder::new();
        assert_eq!(decoder.received(), 0, "new decoder should have 0 received");
        assert_eq!(
            decoder.expected_total(),
            None,
            "new decoder should have no expected total"
        );
        assert!(!decoder.is_complete(), "new decoder should not be complete");
    }

    #[test]
    fn test_mobile_decoder_roundtrip_encode_then_decode() {
        let original = b"UniFFI wrapper roundtrip test data for mobile platforms";
        let chunks = encode_multipart(original, 64);
        assert!(chunks.len() > 1, "should produce multiple chunks");

        let decoder = MobileMultipartDecoder::new();
        for chunk in &chunks {
            let is_new = decoder.add_chunk(chunk.clone()).expect("valid chunk");
            assert!(is_new, "first add of each chunk should return true");
        }

        assert!(
            decoder.is_complete(),
            "all chunks added, should be complete"
        );
        assert_eq!(decoder.received(), chunks.len() as u32);
        assert_eq!(decoder.expected_total(), Some(chunks.len() as u32));

        let assembled = decoder.assemble().expect("assemble should succeed");
        assert_eq!(assembled, original, "reassembled data must match original");
    }

    #[test]
    fn test_mobile_decoder_duplicate_returns_false() {
        let data = b"duplicate test for mobile decoder wrapper";
        let chunks = encode_multipart(data, 32);
        assert!(chunks.len() > 1, "need multiple chunks");

        let decoder = MobileMultipartDecoder::new();

        let first = decoder.add_chunk(chunks[0].clone()).expect("valid chunk");
        assert!(first, "first add should return true");

        let second = decoder.add_chunk(chunks[0].clone()).expect("valid chunk");
        assert!(!second, "duplicate should return false");

        assert_eq!(decoder.received(), 1, "count should still be 1");
    }

    #[test]
    fn test_mobile_decoder_invalid_chunk_returns_error() {
        let decoder = MobileMultipartDecoder::new();
        let result = decoder.add_chunk("not-valid".into());
        assert!(result.is_err(), "invalid chunk should return error");
    }

    #[test]
    fn test_mobile_decoder_assemble_incomplete_returns_error() {
        let data = vec![0u8; 500];
        let chunks = encode_multipart(&data, 64);
        assert!(chunks.len() > 2, "need several chunks");

        let decoder = MobileMultipartDecoder::new();
        decoder.add_chunk(chunks[0].clone()).expect("valid chunk");

        assert!(!decoder.is_complete(), "should not be complete");
        let result = decoder.assemble();
        assert!(result.is_err(), "assembling incomplete data should error");
    }
}
