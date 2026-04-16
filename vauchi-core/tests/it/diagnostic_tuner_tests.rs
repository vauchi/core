// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::diagnostic::tuner::*;

fn make_result(
    id: u32,
    decode_rate: f32,
    avg_latency_ms: f32,
    jitter_ms: f32,
    thermal_events: u32,
) -> TuningResult {
    TuningResult {
        camera_config_id: id,
        qr_config: QrConfig {
            error_correction: ErrorCorrectionLevel::M,
            payload_size_bytes: 472,
            module_size_px: 10,
        },
        decode_rate,
        avg_latency_ms,
        jitter_ms,
        thermal_events,
        frames_total: 60,
        frames_decoded: (60.0 * decode_rate) as u32,
        actual_iso: Some(100),
        actual_exposure_ev: Some(0),
    }
}

// @internal
#[test]
fn score_config_perfect_decode_rate_dominates() {
    let good = make_result(1, 1.0, 5.0, 2.0, 0);
    let bad = make_result(2, 0.2, 5.0, 2.0, 0);
    assert!(score_config(&good) > score_config(&bad));
}

// @internal
#[test]
fn score_config_lower_latency_scores_higher() {
    let fast = make_result(1, 0.9, 2.0, 2.0, 0);
    let slow = make_result(2, 0.9, 50.0, 2.0, 0);
    assert!(score_config(&fast) > score_config(&slow));
}

// @internal
#[test]
fn score_config_lower_jitter_scores_higher() {
    let stable = make_result(1, 0.9, 5.0, 1.0, 0);
    let jittery = make_result(2, 0.9, 5.0, 30.0, 0);
    assert!(score_config(&stable) > score_config(&jittery));
}

// @internal
#[test]
fn score_config_thermal_events_penalise() {
    let cool = make_result(1, 0.9, 5.0, 2.0, 0);
    let hot = make_result(2, 0.9, 5.0, 2.0, 3);
    assert!(score_config(&cool) > score_config(&hot));
}

// @internal
#[test]
fn score_config_deterministic_with_known_inputs() {
    let result = make_result(1, 1.0, 10.0, 5.0, 0);
    let score = score_config(&result);
    // decode_rate: 1.0 * 0.50 = 0.50
    // latency: (1/10) * 300 * 0.30 = 9.0
    // jitter: (1/5) * 30 * 0.20 = 1.2
    // thermal: 0.0
    let expected = 0.50 + 9.0 + 1.2;
    assert!(
        (score - expected).abs() < 0.001,
        "expected {expected}, got {score}"
    );
}

// @internal
#[test]
fn score_config_zero_latency_clamped_to_one() {
    let result = make_result(1, 1.0, 0.0, 1.0, 0);
    let score = score_config(&result);
    assert!(score.is_finite());
    assert!(score > 0.0);
}

// @internal
#[test]
fn rank_configs_returns_sorted_descending() {
    let results = vec![
        make_result(1, 0.5, 10.0, 5.0, 0),
        make_result(2, 1.0, 2.0, 1.0, 0),
        make_result(3, 0.8, 5.0, 3.0, 0),
    ];
    let ranked = rank_configs(&results);
    assert_eq!(ranked.len(), 3);
    assert_eq!(ranked[0].0, 2, "highest scoring config should be first");
    assert!(ranked[0].1 >= ranked[1].1);
    assert!(ranked[1].1 >= ranked[2].1);
}

// @internal
#[test]
fn rank_configs_empty_input() {
    let ranked = rank_configs(&[]);
    assert!(ranked.is_empty());
}

// @internal
#[test]
fn generate_sweep_matrix_respects_iso_range() {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Android,
        device_model: "Pixel 7".into(),
        hardware_level: Some("FULL".into()),
        iso_range: Some((100, 400)),
        exposure_ev_range: Some((-2, 2)),
        af_modes: vec!["FIXED".into(), "CONTINUOUS_PICTURE".into()],
        awb_modes: vec!["AUTO".into(), "DAYLIGHT".into()],
        fps_ranges: vec![(15, 15), (30, 30)],
        max_resolution: (1920, 1080),
    };
    let matrix = generate_sweep_matrix(&profile);
    for config in &matrix.camera_configs {
        if let Some(iso) = config.iso {
            assert!((100..=400).contains(&iso), "ISO {iso} outside range");
        }
    }
    assert!(!matrix.camera_configs.is_empty());
}

// @internal
#[test]
fn generate_sweep_matrix_skips_unsupported_iso() {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Ios,
        device_model: "iPhone SE".into(),
        hardware_level: None,
        iso_range: None,
        exposure_ev_range: Some((-2, 2)),
        af_modes: vec!["auto".into()],
        awb_modes: vec!["auto".into()],
        fps_ranges: vec![(30, 30)],
        max_resolution: (1920, 1440),
    };
    let matrix = generate_sweep_matrix(&profile);
    for config in &matrix.camera_configs {
        assert!(config.iso.is_none(), "ISO should be None when unsupported");
    }
    assert!(!matrix.camera_configs.is_empty());
}

// @internal
#[test]
fn generate_sweep_matrix_includes_qr_configs() {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Android,
        device_model: "Test".into(),
        hardware_level: Some("FULL".into()),
        iso_range: Some((100, 800)),
        exposure_ev_range: Some((-2, 2)),
        af_modes: vec!["FIXED".into()],
        awb_modes: vec!["AUTO".into()],
        fps_ranges: vec![(30, 30)],
        max_resolution: (1920, 1080),
    };
    let matrix = generate_sweep_matrix(&profile);
    assert!(
        matrix.qr_configs.len() >= 4,
        "should have at least one per EC level"
    );
    let has_all_ec = [
        ErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H,
    ]
    .iter()
    .all(|ec| matrix.qr_configs.iter().any(|q| q.error_correction == *ec));
    assert!(has_all_ec, "should cover all EC levels");
}

// @internal
#[test]
fn generate_sweep_matrix_legacy_android_minimal() {
    let profile = DeviceCapabilityProfile {
        platform: Platform::Android,
        device_model: "Old Phone".into(),
        hardware_level: Some("LEGACY".into()),
        iso_range: None,
        exposure_ev_range: Some((-1, 1)),
        af_modes: vec!["CONTINUOUS_PICTURE".into()],
        awb_modes: vec!["AUTO".into()],
        fps_ranges: vec![(30, 30)],
        max_resolution: (1280, 720),
    };
    let matrix = generate_sweep_matrix(&profile);
    assert!(!matrix.camera_configs.is_empty());
    for config in &matrix.camera_configs {
        assert!(config.iso.is_none());
    }
}
