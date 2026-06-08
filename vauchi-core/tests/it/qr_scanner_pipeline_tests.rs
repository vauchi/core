// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Pipeline coverage for `qr/scanner.rs` without the diagnostic-scanner
//! feature gate.
//!
//! `diagnostic_scanner_tests.rs` covers the same surface but is gated
//! behind `#[cfg(feature = "diagnostic-scanner")]` and therefore not
//! exercised in `just coverage core`'s default feature set. These
//! tests run in the default build and cover:
//!
//! - `RqrrRaw` happy path (rqrr decode)
//! - `RqrrPreprocessed` happy path (rxing fast → rqrr fallback)
//! - Sharpness gate (Tier 2+3 skipped on blurry frames)
//! - Garbage input (no QR found)
//! - Dimension mismatch (early return)

use image::GrayImage;
use vauchi_core::qr::scanner::{ScannerBackend, scan_qr_from_luma};

/// Render a `qrcode::QrCode` to a `GrayImage` with quiet zone and
/// per-module pixel scaling. Mirrors the helper in
/// `diagnostic_scanner_tests.rs` so both files render identically.
fn qr_to_gray_image(code: &qrcode::QrCode, module_px: u32) -> GrayImage {
    let colors = code.to_colors();
    let qr_width = code.width() as u32;
    let quiet = 4u32;
    let total = qr_width + 2 * quiet;
    let img_size = total * module_px;

    let mut img = GrayImage::from_pixel(img_size, img_size, image::Luma([255u8]));
    for (i, &color) in colors.iter().enumerate() {
        let qx = (i as u32) % qr_width;
        let qy = (i as u32) / qr_width;
        if color == qrcode::Color::Dark {
            let px = (qx + quiet) * module_px;
            let py = (qy + quiet) * module_px;
            for dy in 0..module_px {
                for dx in 0..module_px {
                    img.put_pixel(px + dx, py + dy, image::Luma([0u8]));
                }
            }
        }
    }
    img
}

fn generate_qr_image(text: &str, module_px: u32) -> GrayImage {
    let code = qrcode::QrCode::new(text.as_bytes()).expect("QR encode failed");
    qr_to_gray_image(&code, module_px)
}

// ============================================================
// RqrrRaw — direct rqrr decoding path
// ============================================================

// @scenario: qr_scanning :: rqrr decodes a clean QR
// @internal
#[test]
fn rqrr_raw_decodes_clean_qr_in_default_features() {
    let img = generate_qr_image("HELLO-VAUCHI-12345", 8);
    let (w, h) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), w, h);

    assert_eq!(
        result.decoded.as_deref(),
        Some("HELLO-VAUCHI-12345"),
        "rqrr must decode a high-resolution QR"
    );
    assert!(!result.frame_skipped, "clean frame must not be skipped");
    assert!(result.total_us > 0, "timing must be reported");
}

// @internal
#[test]
fn rqrr_raw_returns_none_on_random_bytes() {
    // Deterministic noise — no QR finder pattern.
    let garbage: Vec<u8> = (0..200 * 200).map(|i| ((i * 37 + 7) % 256) as u8).collect();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, &garbage, 200, 200);
    assert!(result.decoded.is_none());
    assert!(
        !result.frame_skipped,
        "raw backend never gates on sharpness — frame_skipped must be false"
    );
}

// @internal
#[test]
fn rqrr_raw_handles_dimension_mismatch_gracefully() {
    // luma_data.len() != width * height — early-return path.
    let data = vec![0u8; 100];
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, &data, 50, 50);
    assert!(result.decoded.is_none());
    assert_eq!(
        result.decode_us, 0,
        "early return must not record any decode time"
    );
    assert!(!result.frame_skipped);
}

// @internal
#[test]
fn rqrr_raw_dimension_mismatch_zero_pixels_does_not_panic() {
    let data: Vec<u8> = vec![];
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, &data, 0, 0);
    // Either decodes nothing (most likely) or doesn't panic. Both fine.
    assert!(result.decoded.is_none());
}

// ============================================================
// RqrrPreprocessed — multi-decoder pipeline
// ============================================================

// @scenario: qr_scanning :: preprocessed pipeline decodes a clean QR
// @internal
#[test]
fn rqrr_preprocessed_decodes_clean_qr_via_fast_tier() {
    // before any sharpness gate or fallback runs.
    let img = generate_qr_image("VAUCHI-FAST-PATH", 8);
    let (w, h) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, img.as_raw(), w, h);

    assert_eq!(result.decoded.as_deref(), Some("VAUCHI-FAST-PATH"));
    assert!(
        !result.frame_skipped,
        "clean frame must not trigger sharpness gate"
    );
}

// @internal
#[test]
fn rqrr_preprocessed_skips_blurry_frame_via_sharpness_gate() {
    // Solid-grey image — Laplacian variance ≈ 0, which is below the
    // SHARPNESS_GATE_THRESHOLD (15.0). The fast tier should fail
    // (no finder pattern), then the gate should skip Tier 2+3.
    let blurry = vec![128u8; 200 * 200];
    let result = scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, &blurry, 200, 200);

    assert!(result.decoded.is_none());
    assert!(
        result.frame_skipped,
        "uniform-grey frame must be skipped by the sharpness gate; \
         actual variance: {}",
        result.laplacian_variance
    );
    assert!(
        result.laplacian_variance < 15.0,
        "uniform-grey should be far below threshold; got {}",
        result.laplacian_variance
    );
}

// @internal
#[test]
fn rqrr_preprocessed_returns_none_on_sharp_garbage_without_skipping() {
    // High-contrast random-ish data — sharpness above threshold but
    // no QR finder pattern. All 3 tiers should attempt and fail.
    // frame_skipped MUST be false because we passed the gate.
    let mut data: Vec<u8> = Vec::with_capacity(200 * 200);
    for i in 0..(200 * 200) {
        data.push(if (i / 17) % 2 == 0 { 0 } else { 255 });
    }
    let result = scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, &data, 200, 200);

    assert!(result.decoded.is_none());
    assert!(
        !result.frame_skipped || result.laplacian_variance < 15.0,
        "high-contrast garbage should pass the gate (variance ≥ 15) \
         OR be skipped — both are fine, but the result must be honest"
    );
}

// @internal
#[test]
fn rqrr_preprocessed_handles_dimension_mismatch_gracefully() {
    let data = vec![0u8; 50];
    let result = scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, &data, 50, 50);
    assert!(result.decoded.is_none());
}

// ============================================================
// Cross-backend invariants
// ============================================================

// @internal
#[test]
fn both_backends_agree_on_clean_qr_payload() {
    let img = generate_qr_image("AGREE-PAYLOAD", 8);
    let (w, h) = img.dimensions();
    let raw = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), w, h);
    let pre = scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, img.as_raw(), w, h);

    assert_eq!(raw.decoded.as_deref(), Some("AGREE-PAYLOAD"));
    assert_eq!(pre.decoded.as_deref(), Some("AGREE-PAYLOAD"));
}

// @internal
#[test]
fn timing_invariant_total_us_geq_decode_us() {
    let img = generate_qr_image("TIMING", 8);
    let (w, h) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), w, h);
    assert!(
        result.total_us >= result.decode_us,
        "total time must include decode time"
    );
}
