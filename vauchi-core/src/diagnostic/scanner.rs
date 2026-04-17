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
pub fn scan_qr_from_luma_with_config(
    backend: ScannerBackend,
    luma_data: &[u8],
    width: u32,
    height: u32,
    preprocess_config: &super::preprocess::PreprocessConfig,
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
            use super::preprocess::preprocess_frame;

            let pre = preprocess_frame(img, preprocess_config);
            if pre.skipped {
                return ScanResult {
                    decoded: None,
                    total_us: total_start.elapsed().as_micros() as u64,
                    preprocessing_us: pre.preprocess_time_us,
                    decode_us: 0,
                    frame_skipped: true,
                    laplacian_variance: pre.laplacian_variance,
                };
            }
            let result = decode_rqrr(pre.image);
            ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: pre.preprocess_time_us,
                laplacian_variance: pre.laplacian_variance,
                ..result
            }
        }
    }
}

/// Decode a QR code from a grayscale image using rqrr.
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
