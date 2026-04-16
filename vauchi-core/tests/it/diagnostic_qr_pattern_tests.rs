// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::tuner::*;

// @internal
#[test]
fn generate_qr_test_patterns_covers_all_ec_levels() {
    let patterns = generate_qr_test_patterns();
    assert!(!patterns.is_empty());
    for ec in &[
        ErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H,
    ] {
        assert!(
            patterns.iter().any(|(cfg, _)| cfg.error_correction == *ec),
            "missing EC level {:?}",
            ec
        );
    }
}

// @internal
#[test]
fn generate_qr_test_patterns_payload_matches_config_size() {
    let patterns = generate_qr_test_patterns();
    for (config, data) in &patterns {
        assert_eq!(
            data.len(),
            config.payload_size_bytes,
            "payload length should match config for {:?} {}B",
            config.error_correction,
            config.payload_size_bytes
        );
    }
}

// @internal
#[test]
fn generate_qr_test_patterns_deterministic() {
    let a = generate_qr_test_patterns();
    let b = generate_qr_test_patterns();
    assert_eq!(a.len(), b.len());
    for (i, ((ca, da), (cb, db))) in a.iter().zip(b.iter()).enumerate() {
        assert_eq!(da, db, "pattern {i} should be deterministic");
        assert_eq!(ca.payload_size_bytes, cb.payload_size_bytes);
    }
}

// @internal
#[test]
fn generate_qr_test_patterns_valid_ascii() {
    let patterns = generate_qr_test_patterns();
    for (_, data) in &patterns {
        assert!(
            data.is_ascii(),
            "pattern should be ASCII for QR compatibility"
        );
    }
}
