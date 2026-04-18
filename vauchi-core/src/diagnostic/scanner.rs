// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR scanner backends for diagnostic benchmarking.
//!
//! Provides rqrr-based QR decoding from raw grayscale (Y-plane) camera
//! frames. Multi-decoder fallback pipeline: rxing fast → rqrr → rxing
//! tryHarder, gated by a fast sharpness check to skip expensive fallbacks
//! on blurry frames.
//!
//! Only the first detected QR grid per frame is decoded.

use image::GrayImage;
use serde::{Deserialize, Serialize};

/// Which scanner pipeline to use for decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScannerBackend {
    /// rqrr on raw Y-plane, no preprocessing.
    RqrrRaw,
    /// Multi-decoder pipeline: rxing fast → rqrr → rxing tryHarder.
    /// Tier 2+3 gated on sharpness to avoid wasting time on blurry frames.
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

/// Minimum Laplacian variance for Tier 2+3 fallback decoders.
///
/// Conservative threshold: only gates extremely blurry frames (rapid motion
/// blur, lens transition). Values below ~15 produce images where no decoder
/// can find finder patterns. Values above ~50 risk gating frames that rxing
/// tryHarder could decode with sub-pixel refinement.
///
/// **Not yet validated on device.** Adjust based on diagnostic tuner data
/// from `_private/docs/investigations/` benchmark runs. The threshold is
/// intentionally low to avoid false gating — a missed optimization is
/// cheaper than a missed QR decode.
const SHARPNESS_GATE_THRESHOLD: f32 = 15.0;

/// Decode a QR code from a grayscale (Y-plane) image.
///
/// The `luma_data` must contain exactly `width * height` bytes of 8-bit
/// grayscale pixel data (e.g., the Y-plane from a YUV camera frame).
pub fn scan_qr_from_luma(
    backend: ScannerBackend,
    luma_data: &[u8],
    width: u32,
    height: u32,
) -> ScanResult {
    let total_start = std::time::Instant::now();

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

    match backend {
        ScannerBackend::RqrrRaw => {
            // Single copy: luma_data → owned Vec for GrayImage
            let img = GrayImage::from_raw(width, height, luma_data.to_vec())
                .expect("dims verified above");
            let result = decode_rqrr(img);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: 0,
                ..result
            }
        }
        ScannerBackend::RqrrPreprocessed => {
            // Opt 1: Pass owned Vec directly to rxing (avoids second clone).
            // rxing::detect_in_luma_with_hints takes Vec<u8> by value.
            let fast = decode_rxing_fast(luma_data.to_vec(), width, height);
            if fast.decoded.is_some() {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: 0,
                    ..fast
                };
            }

            // Opt 2+3: Fast sharpness check on subsampled data before
            // committing to expensive Tier 2+3 fallback decoders.
            let sharpness = fast_laplacian_variance(luma_data, width, height);
            if sharpness < SHARPNESS_GATE_THRESHOLD {
                return ScanResult {
                    decoded: None,
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: 0,
                    decode_us: fast.decode_us,
                    frame_skipped: true,
                    laplacian_variance: sharpness,
                };
            }

            // Tier 2: rqrr (different finder-pattern algorithm)
            let img = GrayImage::from_raw(width, height, luma_data.to_vec())
                .expect("dims verified above");
            let rqrr = decode_rqrr(img);
            if rqrr.decoded.is_some() {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: 0,
                    laplacian_variance: sharpness,
                    ..rqrr
                };
            }

            // Tier 3: rxing tryHarder (sub-pixel refinement, V20+ support)
            let hard = decode_rxing_try_harder(luma_data.to_vec(), width, height);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: 0,
                laplacian_variance: sharpness,
                ..hard
            }
        }
        #[cfg(feature = "diagnostic-yolo")]
        ScannerBackend::YoloRqrr => {
            // YOLO detection requires a pre-loaded detector session.
            // Callers should use scan_qr_yolo() instead.
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

/// Decode a QR code from a grayscale (Y-plane) image with custom preprocessing config.
///
/// The preprocess config is accepted for API compatibility with the diagnostic
/// benchmark harness, but is unused by the current rxing/rqrr multi-decoder
/// pipeline (preprocessing hurts decode rate per vendor findings).
#[cfg(feature = "diagnostic-scanner")]
pub fn scan_qr_from_luma_with_config(
    backend: ScannerBackend,
    luma_data: &[u8],
    width: u32,
    height: u32,
    _preprocess_config: &super::preprocess::PreprocessConfig,
) -> ScanResult {
    scan_qr_from_luma(backend, luma_data, width, height)
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
        let (pw, ph) = patch.dimensions();
        let rxing_result = decode_rxing_try_harder(patch.into_raw(), pw, ph);
        if rxing_result.decoded.is_some() {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detection_us,
                decode_us: decode_start.elapsed().as_micros() as u64,
                ..rxing_result
            };
        }
    }

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
///
/// Takes owned `Vec<u8>` to avoid a second clone — rxing consumes the buffer.
fn decode_rxing_fast(luma: Vec<u8>, width: u32, height: u32) -> ScanResult {
    let decode_start = std::time::Instant::now();

    let mut hints = rxing::DecodeHints {
        TryHarder: Some(false),
        ..Default::default()
    };

    let decoded = rxing::helpers::detect_in_luma_with_hints(
        luma,
        width,
        height,
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
/// Takes owned `Vec<u8>` to avoid a second clone — rxing consumes the buffer.
fn decode_rxing_try_harder(luma: Vec<u8>, width: u32, height: u32) -> ScanResult {
    let decode_start = std::time::Instant::now();

    let mut hints = rxing::DecodeHints {
        TryHarder: Some(true),
        ..Default::default()
    };

    let decoded = rxing::helpers::detect_in_luma_with_hints(
        luma,
        width,
        height,
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

/// Fast Laplacian variance on subsampled data — ~15x cheaper than full resolution.
///
/// Samples every 4th pixel in both dimensions (1/16th of total pixels).
/// Sufficient for detecting motion blur without spending 2-5ms on a full
/// 1920×1080 Laplacian. Cost: ~0.1-0.3ms on 1080p.
fn fast_laplacian_variance(luma: &[u8], width: u32, height: u32) -> f32 {
    let w = width as usize;
    let h = height as usize;
    if w < 12 || h < 12 {
        return 0.0;
    }

    let step = 4; // Sample every 4th pixel
    let mut sum = 0i64;
    let mut sum_sq = 0i64;
    let mut count = 0u64;

    // 3×3 Laplacian kernel on subsampled grid: [0,-1,0; -1,4,-1; 0,-1,0]
    // Neighbors are `step` pixels apart in each dimension.
    let y_start = step;
    let y_end = h - step;
    let x_start = step;
    let x_end = w - step;

    let mut y = y_start;
    while y < y_end {
        let mut x = x_start;
        while x < x_end {
            let center = luma[y * w + x] as i32;
            let top = luma[(y - step) * w + x] as i32;
            let bottom = luma[(y + step) * w + x] as i32;
            let left = luma[y * w + (x - step)] as i32;
            let right = luma[y * w + (x + step)] as i32;
            let lap = 4 * center - top - bottom - left - right;
            sum += lap as i64;
            sum_sq += (lap as i64) * (lap as i64);
            count += 1;
            x += step;
        }
        y += step;
    }

    if count == 0 {
        return 0.0;
    }

    let mean = sum as f64 / count as f64;
    let variance = (sum_sq as f64 / count as f64) - (mean * mean);
    variance as f32
}
