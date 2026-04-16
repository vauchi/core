// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::log_event::*;
use vauchi_core::diagnostic::report::generate_html_report;
use vauchi_core::diagnostic::snapshot::*;
use vauchi_core::diagnostic::tuner::*;

fn sample_session_data() -> (
    Vec<TuningResult>,
    Vec<LogEvent>,
    Vec<SnapshotMetadata>,
    DeviceCapabilityProfile,
) {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Android,
        device_model: "Pixel 7".into(),
        hardware_level: Some("FULL".into()),
        iso_range: Some((100, 800)),
        exposure_ev_range: Some((-2, 2)),
        af_modes: vec!["CONTINUOUS_PICTURE".into()],
        awb_modes: vec!["AUTO".into()],
        fps_ranges: vec![(30, 30)],
        max_resolution: (1920, 1080),
    };

    let results = vec![
        TuningResult {
            camera_config_id: 0,
            qr_config: QrConfig {
                error_correction: ErrorCorrectionLevel::M,
                payload_size_bytes: 472,
                module_size_px: 10,
            },
            decode_rate: 0.95,
            avg_latency_ms: 8.0,
            jitter_ms: 2.0,
            thermal_events: 0,
            frames_total: 60,
            frames_decoded: 57,
            actual_iso: Some(200),
            actual_exposure_ev: Some(0),
        },
        TuningResult {
            camera_config_id: 1,
            qr_config: QrConfig {
                error_correction: ErrorCorrectionLevel::H,
                payload_size_bytes: 250,
                module_size_px: 10,
            },
            decode_rate: 0.80,
            avg_latency_ms: 15.0,
            jitter_ms: 5.0,
            thermal_events: 1,
            frames_total: 60,
            frames_decoded: 48,
            actual_iso: Some(400),
            actual_exposure_ev: Some(-1),
        },
    ];

    let events = vec![
        LogEvent {
            timestamp_ms: 1000,
            device_model: "Pixel 7".into(),
            kind: LogEventKind::SweepStarted { total_configs: 2 },
        },
        LogEvent {
            timestamp_ms: 5000,
            device_model: "Pixel 7".into(),
            kind: LogEventKind::DecodeSuccess {
                latency_ms: 8.0,
                frame_index: 1,
            },
        },
        LogEvent {
            timestamp_ms: 60000,
            device_model: "Pixel 7".into(),
            kind: LogEventKind::SweepComplete {
                best_config_id: 0,
                best_score: 0.95,
            },
        },
    ];

    let snapshots = vec![SnapshotMetadata {
        timestamp_ms: 5000,
        config_id: 0,
        qr_config: QrConfig {
            error_correction: ErrorCorrectionLevel::M,
            payload_size_bytes: 472,
            module_size_px: 10,
        },
        frame_index: 1,
        decode_result: true,
        decode_latency_ms: Some(8.0),
        bounding_box: Some(BoundingBox {
            x: 0.2,
            y: 0.3,
            w: 0.4,
            h: 0.4,
        }),
        actual_iso: Some(200),
        actual_exposure_ev: Some(0),
        actual_focus_distance: None,
        redacted: false,
    }];

    (results, events, snapshots, profile)
}

// @internal
#[test]
fn generate_html_report_produces_valid_html() {
    let (results, events, snapshots, profile) = sample_session_data();
    let html =
        generate_html_report(&profile, &results, &events, &snapshots).expect("should generate");
    assert!(html.starts_with("<!DOCTYPE html>"));
    assert!(html.contains("</html>"));
}

// @internal
#[test]
fn generate_html_report_contains_device_info() {
    let (results, events, snapshots, profile) = sample_session_data();
    let html = generate_html_report(&profile, &results, &events, &snapshots).unwrap();
    assert!(html.contains("Pixel 7"));
    assert!(html.contains("Android"));
}

// @internal
#[test]
fn generate_html_report_contains_svg_chart() {
    let (results, events, snapshots, profile) = sample_session_data();
    let html = generate_html_report(&profile, &results, &events, &snapshots).unwrap();
    assert!(html.contains("<svg"));
    assert!(html.contains("</svg>"));
}

// @internal
#[test]
fn generate_html_report_contains_style_tag() {
    let (results, events, snapshots, profile) = sample_session_data();
    let html = generate_html_report(&profile, &results, &events, &snapshots).unwrap();
    assert!(html.contains("<style>"));
    assert!(html.contains("</style>"));
}

// @internal
#[test]
fn generate_html_report_no_external_dependencies() {
    let (results, events, snapshots, profile) = sample_session_data();
    let html = generate_html_report(&profile, &results, &events, &snapshots).unwrap();
    assert!(!html.contains("https://"));
    assert!(!html.contains("http://"));
    assert!(!html.contains("<script src="));
    assert!(!html.contains("<link rel=\"stylesheet\""));
}

// @internal
#[test]
fn generate_html_report_empty_results() {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Ios,
        device_model: "iPhone SE".into(),
        hardware_level: None,
        iso_range: None,
        exposure_ev_range: None,
        af_modes: vec![],
        awb_modes: vec![],
        fps_ranges: vec![],
        max_resolution: (1920, 1440),
    };
    let html = generate_html_report(&profile, &[], &[], &[]).expect("should handle empty");
    assert!(html.contains("iPhone SE"));
    assert!(html.contains("No results"));
}
