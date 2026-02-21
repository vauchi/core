// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multipart QR Codec
//!
//! Encodes large payloads into multiple QR-sized chunks and reassembles them.
//! Enables offline device linking via animated QR code sequences.
//!
//! Chunk format: `{index}/{total}/{crc32_hex_8chars}/{base64url_data}`

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
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
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
        let err_msg = result.unwrap_err();
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
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

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
}
