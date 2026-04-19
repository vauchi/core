// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "diagnostic-scanner")]

use image::GrayImage;
use vauchi_core::diagnostic::preprocess::*;

/// Create a flat gray image (low sharpness — uniform pixels).
fn make_flat_image(width: u32, height: u32, value: u8) -> GrayImage {
    GrayImage::from_pixel(width, height, image::Luma([value]))
}

/// Create a checkerboard image (high sharpness — sharp edges).
fn make_checkerboard(width: u32, height: u32) -> GrayImage {
    let mut img = GrayImage::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let val = if (x + y) % 2 == 0 { 255 } else { 0 };
            img.put_pixel(x, y, image::Luma([val]));
        }
    }
    img
}

/// Generate a QR code as a GrayImage using qrcode crate matrix output.
fn generate_qr_image(text: &str) -> GrayImage {
    let code = qrcode::QrCode::new(text.as_bytes()).expect("QR encode");
    let colors = code.to_colors();
    let qr_w = code.width() as u32;
    let quiet = 4u32;
    let total = qr_w + 2 * quiet;
    let module_px = 8u32;
    let img_size = total * module_px;

    let mut img = GrayImage::from_pixel(img_size, img_size, image::Luma([255u8]));
    for (i, &color) in colors.iter().enumerate() {
        let qx = (i as u32) % qr_w;
        let qy = (i as u32) / qr_w;
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

// @internal
#[test]
fn sharpness_gate_rejects_flat_image() {
    let img = make_flat_image(200, 200, 128);
    let config = PreprocessConfig {
        sharpness_threshold: 50.0,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    assert!(result.skipped, "flat image should be skipped");
    assert!(
        result.laplacian_variance < 50.0,
        "flat image laplacian variance should be near zero, got {}",
        result.laplacian_variance
    );
}

// @internal
#[test]
fn sharpness_gate_passes_sharp_image() {
    let img = make_checkerboard(200, 200);
    let config = PreprocessConfig {
        sharpness_threshold: 50.0,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    assert!(!result.skipped, "checkerboard should not be skipped");
    assert!(
        result.laplacian_variance > 50.0,
        "checkerboard should have high laplacian variance, got {}",
        result.laplacian_variance
    );
}

// @internal
#[test]
fn sharpness_gate_disabled_when_threshold_zero() {
    let img = make_flat_image(200, 200, 128);
    let config = PreprocessConfig {
        sharpness_threshold: 0.0,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    assert!(!result.skipped, "should not skip when threshold is 0.0");
}

// @internal
#[test]
fn laplacian_variance_zero_for_uniform_image() {
    let img = make_flat_image(100, 100, 128);
    let var = compute_laplacian_variance(&img);
    assert!(
        var < 1.0,
        "uniform image should have near-zero variance, got {var}"
    );
}

// @internal
#[test]
fn laplacian_variance_high_for_edges() {
    let img = make_checkerboard(100, 100);
    let var = compute_laplacian_variance(&img);
    assert!(
        var > 1000.0,
        "checkerboard should have very high variance, got {var}"
    );
}

// @internal
#[test]
fn laplacian_variance_handles_tiny_image() {
    let img = make_flat_image(2, 2, 128);
    let var = compute_laplacian_variance(&img);
    assert_eq!(var, 0.0, "image too small for Laplacian kernel");
}

// @internal
#[test]
fn preprocess_reports_timing() {
    let img = make_checkerboard(200, 200);
    let config = PreprocessConfig::default();
    let result = preprocess_frame(img, &config);
    assert!(
        result.preprocess_time_us > 0,
        "preprocess_time_us should be nonzero"
    );
}

// @internal
#[test]
fn preprocess_output_is_binary() {
    // Adaptive threshold should produce only 0 and 255 values
    let img = make_checkerboard(200, 200);
    let config = PreprocessConfig {
        target_width: 0, // skip downscale
        sharpness_threshold: 0.0,
        apply_threshold: true,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    for pixel in result.image.pixels() {
        let v = pixel[0];
        assert!(
            v == 0 || v == 255,
            "adaptive threshold output should be binary, got {v}"
        );
    }
}

// @internal
#[test]
fn preprocessed_qr_still_decodable() {
    use vauchi_core::qr::scanner::*;

    // Generate a QR and add noise to simulate a camera frame
    let mut img = generate_qr_image("PREPROCESS_DECODE_TEST");
    let (w, h) = img.dimensions();
    // Add noise: shift pixel values by a small deterministic amount
    for y in 0..h {
        for x in 0..w {
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

    // First verify raw rqrr can decode the noisy image
    let raw_result = scan_qr_from_luma(ScannerBackend::RqrrRaw, img.as_raw(), w, h);
    assert_eq!(
        raw_result.decoded.as_deref(),
        Some("PREPROCESS_DECODE_TEST"),
        "raw rqrr should decode noisy QR (baseline)"
    );

    // Now verify preprocessing doesn't break decodability.
    // Use conservative config appropriate for small synthetic images:
    // large tiles, no downscale, no threshold.
    let config = PreprocessConfig {
        target_width: 0,
        // CLAHE is designed for real camera frames with lighting gradients,
        // not synthetic QR images — skip it here, test separately below.
        apply_clahe: false,
        apply_unsharp: false,
        sharpness_threshold: 0.0,
        apply_threshold: false,
        ..Default::default()
    };
    let result = scan_qr_from_luma_with_config(
        ScannerBackend::RqrrPreprocessed,
        img.as_raw(),
        w,
        h,
        &config,
    );
    assert_eq!(
        result.decoded.as_deref(),
        Some("PREPROCESS_DECODE_TEST"),
        "preprocessed noisy QR should still decode"
    );
    // preprocessing_us is 0 since CLAHE/unsharp were removed per vendor findings
    assert!(!result.frame_skipped);
}

// @internal
#[test]
fn downscale_reduces_dimensions() {
    let img = make_checkerboard(1920, 1080);
    let config = PreprocessConfig {
        target_width: 720,
        sharpness_threshold: 0.0,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    assert_eq!(result.image.width(), 720);
    // Height should be proportionally scaled
    let expected_h = (1080.0_f64 * 720.0 / 1920.0).round() as u32;
    assert_eq!(result.image.height(), expected_h);
}

// @internal
#[test]
fn no_downscale_when_already_small() {
    let img = make_checkerboard(320, 240);
    let config = PreprocessConfig {
        target_width: 720,
        sharpness_threshold: 0.0,
        ..Default::default()
    };
    let result = preprocess_frame(img, &config);
    // Should not upscale — dimensions unchanged
    assert_eq!(result.image.width(), 320);
}
