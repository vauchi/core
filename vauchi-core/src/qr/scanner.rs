// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Production QR scanner — rqrr + rxing multi-decoder pipeline.
//!
//! Decodes QR codes from raw grayscale (Y-plane) camera frames. Pipeline:
//! rxing fast → rqrr → rxing tryHarder, gated by a fast sharpness check
//! that skips expensive fallbacks on blurry frames.
//!
//! Every code a tier finds in the frame is reported: facing the other
//! phone, a camera can see the peer's code and a reflection of its own,
//! and keeping only the first dropped the peer's whenever the reflection
//! came first (vauchi/private#450).
//!
//! Diagnostic variants (preprocessing-config wrapper, YOLO-based pipeline)
//! remain in `crate::diagnostic` behind the `diagnostic-scanner` and
//! `diagnostic-yolo` features.

use image::GrayImage;
use rxing::multi::MultipleBarcodeReader;
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
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    /// The first code decoded, or None if decode failed.
    pub decoded: Option<String>,
    /// Every distinct code decoded in the frame, in the order found;
    /// `decoded` is its first entry.
    pub decoded_all: Vec<String>,
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

impl ScanResult {
    /// A result carrying `codes`, duplicates dropped, first one first.
    fn with_codes(codes: impl IntoIterator<Item = String>, decode_us: u64) -> Self {
        let mut decoded_all: Vec<String> = Vec::new();
        for code in codes {
            if !decoded_all.contains(&code) {
                decoded_all.push(code);
            }
        }
        Self {
            decoded: decoded_all.first().cloned(),
            decoded_all,
            decode_us,
            ..Self::default()
        }
    }

    pub(crate) fn found_any(&self) -> bool {
        !self.decoded_all.is_empty()
    }
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

/// Decode the QR codes in a grayscale (Y-plane) image.
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
        return ScanResult::default();
    }

    match backend {
        ScannerBackend::RqrrRaw => {
            // Single copy: luma_data → owned Vec for GrayImage
            let img = GrayImage::from_raw(width, height, luma_data.to_vec())
                .expect("dims verified above");
            let result = decode_rqrr(img);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                ..result
            }
        }
        ScannerBackend::RqrrPreprocessed => {
            // Opt 1: Pass owned Vec directly to rxing (avoids second clone).
            let fast = decode_rxing(luma_data.to_vec(), width, height, false);
            if fast.found_any() {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    ..fast
                };
            }

            // Opt 2+3: Fast sharpness check on subsampled data before
            // committing to expensive Tier 2+3 fallback decoders.
            let sharpness = fast_laplacian_variance(luma_data, width, height);
            if sharpness < SHARPNESS_GATE_THRESHOLD {
                return ScanResult {
                    total_us: total_start.elapsed().as_micros() as u64,
                    decode_us: fast.decode_us,
                    frame_skipped: true,
                    laplacian_variance: sharpness,
                    ..ScanResult::default()
                };
            }

            // Tier 2: rqrr (different finder-pattern algorithm), then
            // Tier 3: rxing tryHarder (sub-pixel refinement, V20+ support).
            let img = GrayImage::from_raw(width, height, luma_data.to_vec())
                .expect("dims verified above");
            let rqrr = decode_rqrr(img);
            let decoded = if rqrr.found_any() {
                rqrr
            } else {
                decode_rxing(luma_data.to_vec(), width, height, true)
            };
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                laplacian_variance: sharpness,
                ..decoded
            }
        }
        #[cfg(feature = "diagnostic-yolo")]
        // YOLO detection needs a pre-loaded detector session, which only
        // scan_qr_yolo() takes, so this backend decodes nothing here.
        ScannerBackend::YoloRqrr => ScanResult::default(),
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
    _preprocess_config: &crate::diagnostic::preprocess::PreprocessConfig,
) -> ScanResult {
    scan_qr_from_luma(backend, luma_data, width, height)
}

#[cfg(feature = "diagnostic-yolo")]
pub use crate::diagnostic::yolo_scan::scan_qr_yolo;

/// Decode every QR grid rqrr finds in a grayscale image (fast, simple).
pub(crate) fn decode_rqrr(img: GrayImage) -> ScanResult {
    let decode_start = std::time::Instant::now();
    let mut prepared = rqrr::PreparedImage::prepare(img);
    let codes: Vec<String> = prepared
        .detect_grids()
        .iter()
        .filter_map(|grid| grid.decode().ok().map(|(_, content)| content))
        .collect();
    ScanResult::with_codes(codes, decode_start.elapsed().as_micros() as u64)
}

/// Decode every QR code in the frame with rxing's QR multi reader, which
/// finds all finder-pattern sets in one pass over the binarized frame.
/// `try_harder` adds sub-pixel refinement (V20+ support) at a higher cost;
/// without it, ~10ms on 480p for clean codes like the exchange frames.
///
/// Takes owned `Vec<u8>` to avoid a second clone — rxing consumes the buffer.
pub(crate) fn decode_rxing(luma: Vec<u8>, width: u32, height: u32, try_harder: bool) -> ScanResult {
    let decode_start = std::time::Instant::now();

    let hints = rxing::DecodeHints {
        TryHarder: Some(try_harder),
        ..Default::default()
    };
    let mut bitmap = rxing::BinaryBitmap::new(rxing::common::HybridBinarizer::new(
        rxing::Luma8LuminanceSource::new(luma, width, height),
    ));
    let codes: Vec<String> = rxing::multi::qrcode::QRCodeMultiReader::new()
        .decode_multiple_with_hints(&mut bitmap, &hints)
        .map(|results| results.iter().map(|r| r.getText().to_string()).collect())
        .unwrap_or_default();

    ScanResult::with_codes(codes, decode_start.elapsed().as_micros() as u64)
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
