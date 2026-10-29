// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The production scanner reads real camera frames: the Pixel 3a's front
//! camera looking at an iPhone SE's Hover code about 13.5 cm away, the
//! soft, small codes a fixed-focus lens delivers in the 7–14 cm range.
//! Synthetic codes did not catch a scanner that read a third of these
//! (vauchi/private#450, 2026-10-29).

use std::path::Path;

use vauchi_core::qr_decode::{ScannerBackend, scan_qr_from_luma};

fn frame(name: &str) -> (Vec<u8>, u32, u32) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/qr-camera-frames")
        .join(name);
    let image = image::open(&path)
        .unwrap_or_else(|e| panic!("{name}: {e}"))
        .into_luma8();
    let (width, height) = image.dimensions();
    (image.into_raw(), width, height)
}

/// Frame type and length of each code: the payload carries the session's
/// key material, which a test has no business spelling out.
fn decoded(name: &str) -> Vec<(String, usize)> {
    let (luma, width, height) = frame(name);
    scan_qr_from_luma(ScannerBackend::RqrrPreprocessed, &luma, width, height)
        .decoded_all
        .iter()
        .map(|code| (code.chars().take(4).collect(), code.len()))
        .collect()
}

// @internal
#[test]
fn a_soft_init_frame_from_the_front_camera_is_read() {
    assert_eq!(decoded("f10.png"), [("INI3".to_string(), 147)]);
}

// @internal
#[test]
fn soft_data_frames_from_the_front_camera_are_read() {
    assert_eq!(decoded("f20.png"), [("DAT4".to_string(), 143)]);
    assert_eq!(decoded("f75.png"), [("DAT4".to_string(), 99)]);
    assert_eq!(decoded("f90.png"), [("DAT4".to_string(), 145)]);
}
