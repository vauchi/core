// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Compatibility paths for QR code scanning.
//!
//! New production callers use `crate::qr_decode`. Optional diagnostic
//! wrappers remain in `scanner` for existing consumers.

pub mod scanner;

pub use scanner::{ScanResult, ScannerBackend, scan_qr_from_luma};
