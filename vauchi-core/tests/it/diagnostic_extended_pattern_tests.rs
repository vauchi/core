// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::tuner::*;

// @internal
#[test]
fn extended_patterns_cover_all_ec_levels() {
    let patterns = generate_extended_qr_test_patterns();
    // 4 EC levels × 8 payload sizes × 3 module sizes = 96
    assert_eq!(patterns.len(), 96, "4 EC × 8 sizes × 3 modules");
    for ec in [
        ErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H,
    ] {
        assert!(
            patterns.iter().any(|(cfg, _)| cfg.error_correction == ec),
            "missing EC level {ec:?}"
        );
    }
}

// @internal
#[test]
fn extended_patterns_include_version20_plus_sizes() {
    let patterns = generate_extended_qr_test_patterns();
    let sizes: Vec<usize> = patterns
        .iter()
        .map(|(cfg, _)| cfg.payload_size_bytes)
        .collect();
    // Version 20 capacity at ECC-L is ~1817 bytes, test payload = 1400
    assert!(sizes.contains(&1400), "should include 1400B (Version 20)");
    // Version 30 capacity at ECC-L is ~3057 bytes, test payload = 2400
    assert!(sizes.contains(&2400), "should include 2400B (Version 30)");
    // Version 40 capacity at ECC-L is ~4296 bytes, test payload = 3300
    assert!(sizes.contains(&3300), "should include 3300B (Version 40)");
}

// @internal
#[test]
fn extended_patterns_payload_length_matches_config() {
    let patterns = generate_extended_qr_test_patterns();
    for (config, data) in &patterns {
        assert_eq!(
            data.len(),
            config.payload_size_bytes,
            "payload length mismatch for {config:?}"
        );
    }
}

// @internal
#[test]
fn extended_patterns_are_deterministic() {
    let a = generate_extended_qr_test_patterns();
    let b = generate_extended_qr_test_patterns();
    assert_eq!(a.len(), b.len());
    for (i, ((_, da), (_, db))) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(da, db, "pattern {i} should be deterministic");
    }
}

// @internal
#[test]
fn extended_patterns_are_valid_ascii() {
    let patterns = generate_extended_qr_test_patterns();
    for (_, data) in &patterns {
        assert!(data.is_ascii(), "pattern should be ASCII");
    }
}

// @internal
#[test]
fn extended_patterns_include_original_sizes() {
    let patterns = generate_extended_qr_test_patterns();
    let sizes: Vec<usize> = patterns
        .iter()
        .map(|(cfg, _)| cfg.payload_size_bytes)
        .collect();
    assert!(sizes.contains(&100), "should include original 100B");
    assert!(sizes.contains(&250), "should include original 250B");
    assert!(sizes.contains(&472), "should include original 472B");
}

// @internal
#[test]
fn throughput_sequence_splits_correctly() {
    let total = 50_000;
    let capacity = 1400;
    let frames = generate_throughput_sequence(total, capacity);

    // chunk_size = 1400 - 20 (header budget) = 1380
    // ceil(50000 / 1380) = 37 frames
    assert_eq!(frames.len(), 37, "50000 bytes / 1380 chunk = 37 frames");
}

// @internal
#[test]
fn throughput_sequence_frame_indices_are_sequential() {
    let frames = generate_throughput_sequence(5000, 500);
    for (i, frame) in frames.iter().enumerate() {
        assert_eq!(
            frame.frame_index, i as u32,
            "frame index mismatch at position {i}"
        );
        assert_eq!(
            frame.total_frames,
            frames.len() as u32,
            "total_frames mismatch"
        );
    }
}

// @internal
#[test]
fn throughput_sequence_data_has_seq_header() {
    let frames = generate_throughput_sequence(5000, 500);
    for frame in &frames {
        assert!(
            frame.data.starts_with("SEQ:"),
            "frame data should start with SEQ: header"
        );
    }
}

// @internal
#[test]
fn throughput_sequence_reassembles_to_original() {
    let total = 10_000;
    let capacity = 800;
    let frames = generate_throughput_sequence(total, capacity);

    let mut reassembled = String::new();
    for frame in &frames {
        // Parse "SEQ:{idx}:{total}:{data}"
        let parts: Vec<&str> = frame.data.splitn(4, ':').collect();
        assert_eq!(parts.len(), 4, "frame should have 4 parts");
        assert_eq!(parts[0], "SEQ");
        reassembled.push_str(parts[3]);
    }

    assert_eq!(reassembled.len(), total);
    // Verify it matches the deterministic payload
    let expected: String = (0..total)
        .map(|i| {
            let byte = ((i * 7 + 13) % 62) as u8;
            match byte {
                0..=9 => (b'0' + byte) as char,
                10..=35 => (b'A' + byte - 10) as char,
                36..=61 => (b'a' + byte - 36) as char,
                _ => unreachable!(),
            }
        })
        .collect();
    assert_eq!(reassembled, expected);
}

// @internal
#[test]
fn throughput_sequence_frames_fit_capacity() {
    let frames = generate_throughput_sequence(50_000, 1400);
    for frame in &frames {
        assert!(
            frame.data.len() <= 1400,
            "frame data {}B exceeds capacity 1400B",
            frame.data.len()
        );
    }
}

// @internal
#[test]
fn throughput_sequence_is_deterministic() {
    let a = generate_throughput_sequence(10_000, 800);
    let b = generate_throughput_sequence(10_000, 800);
    assert_eq!(a.len(), b.len());
    for (i, (fa, fb)) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(fa.data, fb.data, "frame {i} not deterministic");
    }
}
