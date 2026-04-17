// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "diagnostic-scanner")]

use image::GrayImage;
use vauchi_core::diagnostic::scanner::*;

/// Convert a QrCode matrix to a GrayImage with quiet zone and module scaling.
fn qr_to_gray_image(code: &qrcode::QrCode, module_px: u32) -> GrayImage {
    let colors = code.to_colors();
    let qr_width = code.width() as u32;
    let quiet = 4u32; // standard quiet zone in modules
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

/// Generate a QR code image from text and return it as a GrayImage.
fn generate_qr_image(text: &str) -> GrayImage {
    let code = qrcode::QrCode::new(text.as_bytes()).expect("QR encode failed");
    qr_to_gray_image(&code, 8)
}

/// Generate a QR code image letting the crate pick the version.
fn generate_auto_qr_image(text: &str) -> GrayImage {
    let code = qrcode::QrCode::new(text.as_bytes()).expect("QR encode failed");
    qr_to_gray_image(&code, 4)
}

// @internal
#[test]
fn rqrr_raw_decodes_simple_qr() {
    let img = generate_qr_image("HELLO12345");
    let (width, height) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), width, height);
    assert_eq!(
        result.decoded.as_deref(),
        Some("HELLO12345"),
        "should decode simple QR"
    );
    assert!(
        !result.frame_skipped,
        "valid QR frame should not be skipped"
    );
}

// @internal
#[test]
fn rqrr_raw_decodes_large_qr() {
    // 250 bytes in binary mode requires Version 11+ (auto-selected)
    let payload: String = (0..250)
        .map(|i| {
            let b = ((i * 7 + 13) % 62) as u8;
            match b {
                0..=9 => (b'0' + b) as char,
                10..=35 => (b'A' + b - 10) as char,
                36..=61 => (b'a' + b - 36) as char,
                _ => unreachable!(),
            }
        })
        .collect();
    let img = generate_auto_qr_image(&payload);
    let (width, height) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), width, height);
    assert_eq!(
        result.decoded.as_deref(),
        Some(payload.as_str()),
        "should decode large QR"
    );
}

// @internal
#[test]
fn rqrr_raw_returns_none_on_garbage() {
    // Random-ish bytes that won't form a valid QR
    let garbage: Vec<u8> = (0..100 * 100).map(|i| ((i * 37 + 7) % 256) as u8).collect();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, &garbage, 100, 100);
    assert!(result.decoded.is_none(), "should not decode garbage data");
    assert!(
        !result.frame_skipped,
        "valid QR frame should not be skipped"
    );
}

// @internal
#[test]
fn rqrr_raw_reports_timing() {
    let img = generate_qr_image("TIMING_TEST");
    let (width, height) = img.dimensions();
    let result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), width, height);
    assert!(result.decode_us > 0, "decode_us should be nonzero");
    assert!(result.total_us > 0, "total_us should be nonzero");
    assert!(
        result.total_us >= result.decode_us,
        "total_us should be >= decode_us"
    );
}

// @internal
#[test]
fn rqrr_raw_handles_dimension_mismatch() {
    // Data length doesn't match width * height
    let data = vec![0u8; 100];
    let result = scan_qr_from_luma(
        ScannerBackend::RqrrRaw,
        &data,
        50,
        50, // expects 2500 bytes, got 100
    );
    assert!(
        result.decoded.is_none(),
        "should handle dimension mismatch gracefully"
    );
}

// @internal
#[test]
fn rqrr_preprocessed_decodes_noisy_qr() {
    use vauchi_core::diagnostic::preprocess::PreprocessConfig;

    // Add noise to simulate a camera frame
    let mut img = generate_qr_image("PREPROCESS_TEST");
    let (width, height) = img.dimensions();
    for y in 0..height {
        for x in 0..width {
            let v = img.get_pixel(x, y)[0];
            let noise = ((x * 7 + y * 13) % 30) as u8;
            let noisy = if v > 128 {
                255u8.saturating_sub(noise)
            } else {
                noise
            };
            img.put_pixel(x, y, image::Luma([noisy]));
        }
    }
    // Skip CLAHE/unsharp for synthetic images (designed for real camera frames)
    let config = PreprocessConfig {
        target_width: 0,
        sharpness_threshold: 0.0,
        apply_clahe: false,
        apply_unsharp: false,
        apply_threshold: false,
        ..Default::default()
    };
    let result = scan_qr_from_luma_with_config(
        ScannerBackend::RqrrPreprocessed,
        img.as_raw(),
        width,
        height,
        &config,
    );
    assert_eq!(
        result.decoded.as_deref(),
        Some("PREPROCESS_TEST"),
        "preprocessed noisy QR should decode"
    );
}
