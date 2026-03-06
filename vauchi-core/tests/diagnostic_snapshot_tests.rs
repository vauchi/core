// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::snapshot::{BoundingBox, SnapshotMetadata};
use vauchi_core::diagnostic::tuner::{ErrorCorrectionLevel, QrConfig};

#[test]
fn snapshot_metadata_roundtrips_json() {
    let meta = SnapshotMetadata {
        timestamp_ms: 1709712000000,
        config_id: 42,
        qr_config: QrConfig {
            error_correction: ErrorCorrectionLevel::H,
            payload_size_bytes: 472,
            module_size_px: 10,
        },
        frame_index: 7,
        decode_result: true,
        decode_latency_ms: Some(8.3),
        bounding_box: Some(BoundingBox {
            x: 0.1,
            y: 0.2,
            w: 0.5,
            h: 0.5,
        }),
        actual_iso: Some(200),
        actual_exposure_ev: Some(-1),
        actual_focus_distance: Some(0.15),
        redacted: false,
    };

    let json = serde_json::to_string_pretty(&meta).expect("serialize");
    let back: SnapshotMetadata = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.config_id, 42);
    assert_eq!(back.decode_result, true);
    assert_eq!(back.actual_iso, Some(200));
    assert!(!back.redacted);
}

#[test]
fn snapshot_metadata_handles_none_fields() {
    let meta = SnapshotMetadata {
        timestamp_ms: 0,
        config_id: 0,
        qr_config: QrConfig {
            error_correction: ErrorCorrectionLevel::L,
            payload_size_bytes: 100,
            module_size_px: 6,
        },
        frame_index: 0,
        decode_result: false,
        decode_latency_ms: None,
        bounding_box: None,
        actual_iso: None,
        actual_exposure_ev: None,
        actual_focus_distance: None,
        redacted: false,
    };

    let json = serde_json::to_string(&meta).expect("serialize");
    let back: SnapshotMetadata = serde_json::from_str(&json).expect("deserialize");
    assert!(back.decode_latency_ms.is_none());
    assert!(back.bounding_box.is_none());
}
