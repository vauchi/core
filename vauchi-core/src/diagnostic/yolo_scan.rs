// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The YOLO detect → crop → decode pipeline, compiled only with the
//! `diagnostic-yolo` feature, so it lives with the other diagnostic modules
//! that the mutation build never compiles.

use image::GrayImage;

use crate::qr::scanner::{ScanResult, decode_rqrr, decode_rxing};

/// Scan a QR code using YOLO detection → crop → rqrr decode pipeline.
///
/// The detector locates QR code regions in the frame, crops each one with
/// padding, and feeds the cropped patch to rqrr for decoding. Returns the
/// codes of the first patch that decodes.
pub fn scan_qr_yolo(
    detector: &mut crate::diagnostic::yolo_detector::YoloDetector,
    luma_data: &[u8],
    width: u32,
    height: u32,
    confidence_threshold: f32,
) -> ScanResult {
    let total_start = std::time::Instant::now();

    let expected = (width as usize) * (height as usize);
    if luma_data.len() != expected {
        return ScanResult {
            total_us: total_start.elapsed().as_micros() as u64,
            ..ScanResult::default()
        };
    }
    let img = GrayImage::from_raw(width, height, luma_data.to_vec()).expect("dims verified above");

    let detect_start = std::time::Instant::now();
    let detections = match detector.detect(&img, confidence_threshold) {
        Ok(d) => d,
        Err(_) => {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detect_start.elapsed().as_micros() as u64,
                ..ScanResult::default()
            };
        }
    };
    let detection_us = detect_start.elapsed().as_micros() as u64;

    if detections.is_empty() {
        return ScanResult {
            total_us: total_start.elapsed().as_micros() as u64,
            preprocessing_us: detection_us,
            ..ScanResult::default()
        };
    }

    let decode_start = std::time::Instant::now();
    for det in &detections {
        let patch = crate::diagnostic::yolo_detector::crop_detection(&img, det, 0.15);

        let rqrr_result = decode_rqrr(patch.clone());
        if rqrr_result.found_any() {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detection_us,
                decode_us: decode_start.elapsed().as_micros() as u64,
                ..rqrr_result
            };
        }

        // Fallback: rxing with tryHarder (handles V20+, perspective)
        let (pw, ph) = patch.dimensions();
        let rxing_result = decode_rxing(patch.into_raw(), pw, ph, true);
        if rxing_result.found_any() {
            return ScanResult {
                total_us: total_start.elapsed().as_micros() as u64,
                preprocessing_us: detection_us,
                decode_us: decode_start.elapsed().as_micros() as u64,
                ..rxing_result
            };
        }
    }

    ScanResult {
        total_us: total_start.elapsed().as_micros() as u64,
        preprocessing_us: detection_us,
        decode_us: decode_start.elapsed().as_micros() as u64,
        ..ScanResult::default()
    }
}
