// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The production scanner reads real camera frames: the Pixel 3a's front
//! camera looking at an iPhone SE's Hover code at both ends of the 7–14 cm
//! target. Generated codes did not catch a detector swap that read 9 of 40
//! such frames instead of 30, and Hover failed 17 of 17 at 13.5 cm
//! (vauchi/private#450, #931).
//!
//! Each placement keeps a floor: how many of its frames the scanner must
//! read. zbar reads every frame in the corpus, so a floor may rise to the
//! corpus size and must never fall.

use std::{collections::BTreeMap, fs, path::PathBuf};

use sha2::{Digest, Sha256};
use vauchi_core::qr_decode::{ScannerBackend, scan_qr_from_luma};

fn corpus() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/qr-camera-corpus")
}

/// Frame type, length and the first 8 hex of the SHA-256: enough to tell
/// a read from a misread without spelling out the session's key material.
fn tag(code: &str) -> String {
    let digest = Sha256::digest(code.as_bytes());
    let hash: String = digest[..4].iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}:{}:{hash}",
        code.chars().take(4).collect::<String>(),
        code.len()
    )
}

/// What zbar read in each frame of `placement`, by frame path.
fn expected(placement: &str) -> BTreeMap<String, String> {
    fs::read_to_string(corpus().join("expected.txt"))
        .expect("expected.txt")
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .filter_map(|line| line.split_once(' '))
        .filter(|(frame, _)| frame.starts_with(&format!("{placement}/")))
        .map(|(frame, tag)| (frame.to_string(), tag.to_string()))
        .collect()
}

fn scanned(frame: &str) -> Vec<String> {
    let image = image::open(corpus().join(frame))
        .unwrap_or_else(|e| panic!("{frame}: {e}"))
        .into_luma8();
    let (width, height) = image.dimensions();
    scan_qr_from_luma(
        ScannerBackend::RqrrPreprocessed,
        image.as_raw(),
        width,
        height,
    )
    .decoded_all
    .iter()
    .map(|code| tag(code))
    .collect()
}

struct Reading {
    read: usize,
    missed: Vec<String>,
    misread: Vec<String>,
}

fn read_placement(placement: &str) -> Reading {
    let frames = expected(placement);
    assert!(
        frames.len() >= 10,
        "{placement}: corpus has {} frames",
        frames.len()
    );
    let mut reading = Reading {
        read: 0,
        missed: Vec::new(),
        misread: Vec::new(),
    };
    for (frame, want) in &frames {
        let got = scanned(frame);
        if got.contains(want) {
            reading.read += 1;
        } else {
            reading.missed.push(frame.clone());
        }
        reading.misread.extend(
            got.iter()
                .filter(|tag| *tag != want)
                .map(|tag| format!("{frame}: {tag}")),
        );
    }
    reading
}

// @internal
#[test]
fn front_camera_frames_at_7_cm_are_read_at_least_as_well_as_before() {
    const FLOOR: usize = 8;

    let reading = read_placement("pixel3a-front-7cm");

    assert!(
        reading.read >= FLOOR,
        "read {} of 10, floor {FLOOR}; missed {:?}",
        reading.read,
        reading.missed
    );
    assert_eq!(reading.misread, Vec::<String>::new());
}

// @internal
#[test]
fn front_camera_frames_at_10_cm_are_read_at_least_as_well_as_before() {
    const FLOOR: usize = 9;

    let reading = read_placement("pixel3a-front-10cm");

    assert!(
        reading.read >= FLOOR,
        "read {} of 10, floor {FLOOR}; missed {:?}",
        reading.read,
        reading.missed
    );
    assert_eq!(reading.misread, Vec::<String>::new());
}

// @internal
#[test]
fn front_camera_frames_at_13_5_cm_are_read_at_least_as_well_as_before() {
    const FLOOR: usize = 9;

    let reading = read_placement("pixel3a-front-13.5cm");

    assert!(
        reading.read >= FLOOR,
        "read {} of 10, floor {FLOOR}; missed {:?}",
        reading.read,
        reading.missed
    );
    assert_eq!(reading.misread, Vec::<String>::new());
}

// @internal
#[test]
fn front_camera_frames_at_14_cm_are_read_at_least_as_well_as_before() {
    const FLOOR: usize = 9;

    let reading = read_placement("pixel3a-front-14cm");

    assert!(
        reading.read >= FLOOR,
        "read {} of 10, floor {FLOOR}; missed {:?}",
        reading.read,
        reading.missed
    );
    assert_eq!(reading.misread, Vec::<String>::new());
}
