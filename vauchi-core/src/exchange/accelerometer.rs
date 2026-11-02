// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Accelerometer tap/shake correlation for the multi-stage exchange.
//!
//! Both devices record accelerometer data during a shared physical action
//! (table tap, phone bump, shake). The shell records the samples (ADR-031);
//! Core cross-correlates the two magnitude envelopes, and above the
//! threshold the devices felt the same impulse.

/// Configuration for accelerometer verification.
#[derive(Debug, Clone)]
pub struct AccelerometerConfig {
    /// Duration to record accelerometer data (ms). Default: 3000.
    pub recording_duration_ms: u32,
    /// Accelerometer sample rate (Hz). Default: 100.
    pub sample_rate_hz: u32,
    /// Minimum cross-correlation coefficient to accept. Default: 0.6.
    pub correlation_threshold: f32,
}

impl Default for AccelerometerConfig {
    fn default() -> Self {
        AccelerometerConfig {
            recording_duration_ms: 3000,
            sample_rate_hz: 100,
            correlation_threshold: 0.6,
        }
    }
}

/// Normalized cross-correlation (Pearson correlation coefficient).
///
/// Returns a value in [-1.0, 1.0]. Signals are truncated to the shorter length.
/// Returns 0.0 for constant or empty signals.
pub fn cross_correlate(a: &[f32], b: &[f32]) -> f32 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let a = &a[..len];
    let b = &b[..len];

    let mean_a: f64 = a.iter().map(|&x| x as f64).sum::<f64>() / len as f64;
    let mean_b: f64 = b.iter().map(|&x| x as f64).sum::<f64>() / len as f64;

    let mut cov = 0.0f64;
    let mut var_a = 0.0f64;
    let mut var_b = 0.0f64;

    for i in 0..len {
        let da = a[i] as f64 - mean_a;
        let db = b[i] as f64 - mean_b;
        cov += da * db;
        var_a += da * da;
        var_b += db * db;
    }

    let denom = (var_a * var_b).sqrt();
    if denom < 1e-12 {
        return 0.0; // Constant signal
    }

    (cov / denom) as f32
}
