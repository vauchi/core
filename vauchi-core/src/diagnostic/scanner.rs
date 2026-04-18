// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR scanner backends for diagnostic benchmarking.
//!
//! Provides rqrr-based QR decoding from raw grayscale (Y-plane) camera
//! frames, with optional Tier 1 preprocessing (CLAHE, adaptive threshold,
//! sharpness gating). Intended for future UniFFI export via vauchi-platform
//! for on-device A/B testing against platform-native scanners.
//!
//! Only the first detected QR grid per frame is decoded.

use image::GrayImage;
use serde::{Deserialize, Serialize};

/// Which scanner pipeline to use for decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScannerBackend {
    /// rqrr on raw Y-plane, no preprocessing.
    RqrrRaw,
    /// rqrr with Tier 1 preprocessing pipeline.
    RqrrPreprocessed,
    /// YOLO detector → crop → rqrr decode.
    #[cfg(feature = "diagnostic-yolo")]
    YoloRqrr,
}

/// Result of a single QR scan attempt with timing breakdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    /// Decoded QR content, or None if decode failed.
    pub decoded: Option<String>,
    /// Total scan time in microseconds.
    pub total_us: u64,
    /// Time spent on preprocessing in microseconds (0 for raw).
    pub preprocessing_us: u64,
    /// Time spent on rqrr decode in microseconds.
    pub decode_us: u64,
    /// Whether the frame was skipped by sharpness gating.
    pub frame_skipped: bool,
    /// Laplacian variance (sharpness metric). 0.0 if not computed.
    pub laplacian_variance: f32,
}

/// Decode a QR code from a grayscale (Y-plane) image using default config.
///
/// The `luma_data` must contain exactly `width * height` bytes of 8-bit
/// grayscale pixel data (e.g., the Y-plane from a YUV camera frame).
pub fn scan_qr_from_luma(
    backend: ScannerBackend,
    luma_data: &[u8],
    width: u32,
    height: u32,
) -> ScanResult {
    use super::preprocess::PreprocessConfig;
    scan_qr_from_luma_with_config(
        backend,
        luma_data,
        width,
        height,
        &PreprocessConfig::default(),
    )
}

/// Decode a QR code from a grayscale (Y-plane) image with custom preprocessing config.
#[cfg(feature = "diagnostic-scanner")]
pub fn scan_qr_from_luma_with_config(
    backend: ScannerBackend,
    luma_data: &[u8],
    width: u32,
    height: u32,
    _preprocess_config: &super::preprocess::PreprocessConfig,
) -> ScanResult {
    let total_start = std::time::Instant::now();

    let Some(img) = GrayImage::from_raw(width, height, luma_data.to_vec()) else {
        return ScanResult {
            decoded: None,
            total_us: total_start.elapsed().as_micros() as u64,
            preprocessing_us: 0,
            decode_us: 0,
            frame_skipped: false,
            laplacian_variance: 0.0,
        };
    };

    match backend {
        ScannerBackend::RqrrRaw => {
            let result = decode_rqrr(img);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: 0,
                ..result
            }
        }
        ScannerBackend::RqrrPreprocessed => {
            // Multi-decoder pipeline optimized for animated V4 QR:
            // 1. rxing fast (no tryHarder) — handles simple codes in ~10ms
            // 2. rqrr fallback — different finder-pattern algorithm
            // 3. rxing tryHarder — last resort, sub-pixel refinement
            let fast = decode_rxing_fast(&img);
            if fast.decoded.is_some() {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: 0,
                    ..fast
                };
            }
            let rqrr = decode_rqrr(img.clone());
            if rqrr.decoded.is_some() {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: 0,
                    ..rqrr
                };
            }
            let hard = decode_rxing_try_harder(&img);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: 0,
                ..hard
            }
        }
        #[cfg(feature = "diagnostic-yolo")]
        ScannerBackend::YoloRqrr => {
            // YOLO detection requires a pre-loaded detector session.
            // For the scan_qr_from_luma API, use the standalone function below.
            // This arm returns a stub — callers should use scan_qr_yolo() instead.
            ScanResult {
                decoded: None,
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: 0,
                decode_us: 0,
                frame_skipped: false,
                laplacian_variance: 0.0,
            }
        }
    }
}

/// Scan a QR code using YOLO detection → crop → rqrr decode pipeline.
///
/// The detector locates QR code regions in the frame, crops each one with
/// padding, and feeds the cropped patch to rqrr for decoding. Returns the
/// first successfully decoded QR content.
#[cfg(feature = "diagnostic-yolo")]
pub fn scan_qr_yolo(
    detector: &mut super::yolo_detector::YoloDetector,
    luma_data: &[u8],
    width: u32,
    height: u32,
    confidence_threshold: f32,
) -> ScanResult {
    let total_start = std::time::Instant::now();

    // Build GrayImage — use from_raw with the owned vec only once
    let expected = (width as usize) * (height as usize);
    if luma_data.len() != expected {
        return ScanResult {
            decoded: None,
            total_us: total_start.elapsed().as_micros() as u64,
            preprocessing_us: 0,
            decode_us: 0,
            frame_skipped: false,
            laplacian_variance: 0.0,
        };
    }
    let img = GrayImage::from_raw(width, height, luma_data.to_vec()).expect("dims verified above");

    // Step 1: YOLO detection (uses pre-allocated buffer internally)
    let detect_start = std::time::Instant::now();
    let detections = match detector.detect(&img, confidence_threshold) {
        Ok(d) => d,
        Err(_) => {
            return ScanResult {
                decoded: None,
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detect_start.elapsed().as_micros() as u64,
                decode_us: 0,
                frame_skipped: false,
                laplacian_variance: 0.0,
            };
        }
    };
    let detection_us = detect_start.elapsed().as_micros() as u64;

    if detections.is_empty() {
        return ScanResult {
            decoded: None,
            total_us: total_start.elapsed().as_micros() as u64,
            preprocessing_us: detection_us,
            decode_us: 0,
            frame_skipped: false,
            laplacian_variance: 0.0,
        };
    }

    // Step 2: For each detection, crop → multi-decoder attempt
    // Strategy per vendor findings: no CLAHE, no unsharp (both hurt QR).
    // Try rqrr first (fast), then rxing with tryHarder (handles V20+).
    let decode_start = std::time::Instant::now();
    for det in &detections {
        let patch = super::yolo_detector::crop_detection(&img, det, 0.15);

        // Fast path: rqrr raw decode
        let rqrr_result = decode_rqrr(patch.clone());
        if rqrr_result.decoded.is_some() {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detection_us,
                decode_us: decode_start.elapsed().as_micros() as u64,
                ..rqrr_result
            };
        }

        // Fallback: rxing with tryHarder (handles V20+, perspective)
        let rxing_result = decode_rxing_try_harder(&patch);
        if rxing_result.decoded.is_some() {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detection_us,
                decode_us: decode_start.elapsed().as_micros() as u64,
                ..rxing_result
            };
        }
    }

    // No detection decoded successfully
    ScanResult {
        decoded: None,
        total_us: total_start.elapsed().as_micros() as u64,
        preprocessing_us: detection_us,
        decode_us: decode_start.elapsed().as_micros() as u64,
        frame_skipped: false,
        laplacian_variance: 0.0,
    }
}

/// Decode a QR code from a grayscale image using rqrr (fast, simple).
fn decode_rqrr(img: GrayImage) -> ScanResult {
    let decode_start = std::time::Instant::now();
    let mut prepared = rqrr::PreparedImage::prepare(img);
    let grids = prepared.detect_grids();
    let decoded = grids.first().and_then(|g| {
        let (_, content) = g.decode().ok()?;
        Some(content)
    });
    let decode_us = decode_start.elapsed().as_micros() as u64;

    ScanResult {
        decoded,
        total_us: 0, // set by caller
        preprocessing_us: 0,
        decode_us,
        frame_skipped: false,
        laplacian_variance: 0.0,
    }
}

/// Fast rxing decode without tryHarder — optimized for clean, simple QR
/// codes like animated V4 frames. ~10ms on 480p.
fn decode_rxing_fast(img: &GrayImage) -> ScanResult {
    let decode_start = std::time::Instant::now();
    let (w, h) = img.dimensions();
    let luma = img.as_raw().clone();

    let mut hints = rxing::DecodeHints {
        TryHarder: Some(false),
        ..Default::default()
    };

    let decoded = rxing::helpers::detect_in_luma_with_hints(
        luma,
        w,
        h,
        Some(rxing::BarcodeFormat::QR_CODE),
        &mut hints,
    )
    .ok()
    .map(|r| r.getText().to_string());

    let decode_us = decode_start.elapsed().as_micros() as u64;
    ScanResult {
        decoded,
        total_us: 0,
        preprocessing_us: 0,
        decode_us,
        frame_skipped: false,
        laplacian_variance: 0.0,
    }
}

/// Decode a QR code using rxing with tryHarder hints.
///
/// rxing is a Rust port of ZXing with better Version 20+ support and
/// multi-path decoding. tryHarder enables sub-pixel refinement and
/// additional finder-pattern search strategies.
fn decode_rxing_try_harder(img: &GrayImage) -> ScanResult {
    let decode_start = std::time::Instant::now();
    let (w, h) = img.dimensions();
    let luma = img.as_raw().clone();

    let mut hints = rxing::DecodeHints {
        TryHarder: Some(true),
        ..Default::default()
    };

    let decoded = rxing::helpers::detect_in_luma_with_hints(
        luma,
        w,
        h,
        Some(rxing::BarcodeFormat::QR_CODE),
        &mut hints,
    )
    .ok()
    .map(|r| r.getText().to_string());

    let decode_us = decode_start.elapsed().as_micros() as u64;

    ScanResult {
        decoded,
        total_us: 0,
        preprocessing_us: 0,
        decode_us,
        frame_skipped: false,
        laplacian_variance: 0.0,
    }
}
