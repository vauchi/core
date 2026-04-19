// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Production QR code scanning.
//!
//! rxing/rqrr-based decoder pipeline used by Android and iOS mobile
//! scanners. Always compiled — do not gate behind any feature. Diagnostic
//! variants (preprocessing, YOLO) live in `crate::diagnostic` behind the
//! `diagnostic-scanner` / `diagnostic-yolo` features.

pub mod scanner;

pub use scanner::{ScanResult, ScannerBackend, scan_qr_from_luma};
