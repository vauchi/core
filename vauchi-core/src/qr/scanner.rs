// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Compatibility scanner API and optional diagnostic adapters.
//!
//! Production decoding lives in `crate::qr_decode`; diagnostic pipelines
//! consume that module directly to avoid depending on this facade.

pub use crate::qr_decode::{ScanResult, ScannerBackend, scan_qr_from_luma};

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
