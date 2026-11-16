// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

mod qr_scanner_pipeline {
    use image::GrayImage;
    use vauchi_core::qr::scanner::{ScannerBackend, scan_qr_from_luma};

    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/shared/qr_scanner_pipeline.rs"
    ));
}
