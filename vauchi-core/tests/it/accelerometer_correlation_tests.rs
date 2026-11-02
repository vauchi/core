// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tap/shake correlation the multi-stage exchange runs on the shell's
//! accelerometer samples: its threshold defaults and `cross_correlate`.

#![cfg(feature = "testing")]

use vauchi_core::exchange::accelerometer::AccelerometerConfig;

// ===== AccelerometerConfig defaults =====

// @internal
#[test]
fn default_config_has_sane_values() {
    let config = AccelerometerConfig::default();
    assert_eq!(config.recording_duration_ms, 3000);
    assert_eq!(config.sample_rate_hz, 100);
    assert!(
        config.correlation_threshold > 0.3 && config.correlation_threshold < 1.0,
        "Threshold must be between 0.3 and 1.0, got {}",
        config.correlation_threshold
    );
}

// ===== Cross-correlation math =====

// @internal
#[test]
fn identical_envelopes_have_correlation_one() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    let signal: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
    let corr = cross_correlate(&signal, &signal);
    assert!(
        (corr - 1.0).abs() < 0.01,
        "Self-correlation should be ~1.0, got {}",
        corr
    );
}

// @internal
#[test]
fn opposite_envelopes_have_negative_correlation() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    let signal: Vec<f32> = (0..100).map(|i| (i as f32 * 0.1).sin()).collect();
    let opposite: Vec<f32> = signal.iter().map(|&x| -x).collect();
    let corr = cross_correlate(&signal, &opposite);
    assert!(
        corr < -0.9,
        "Opposite signals should have correlation < -0.9, got {}",
        corr
    );
}

// @internal
#[test]
fn uncorrelated_signals_near_zero() {
    use vauchi_core::exchange::accelerometer::cross_correlate;

    // Two signals with different frequencies
    let a: Vec<f32> = (0..300).map(|i| (i as f32 * 0.1).sin()).collect();
    let b: Vec<f32> = (0..300).map(|i| (i as f32 * 0.37).sin()).collect();
    let corr = cross_correlate(&a, &b);
    assert!(
        corr.abs() < 0.5,
        "Uncorrelated signals should have low correlation, got {}",
        corr
    );
}
