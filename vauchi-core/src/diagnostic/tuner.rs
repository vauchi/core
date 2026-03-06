// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Android,
    Ios,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ErrorCorrectionLevel {
    L,
    M,
    Q,
    H,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCapabilityProfile {
    pub platform: Platform,
    pub device_model: String,
    pub hardware_level: Option<String>,
    pub iso_range: Option<(i32, i32)>,
    pub exposure_ev_range: Option<(i32, i32)>,
    pub af_modes: Vec<String>,
    pub awb_modes: Vec<String>,
    pub fps_ranges: Vec<(i32, i32)>,
    pub max_resolution: (u32, u32),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub id: u32,
    pub iso: Option<i32>,
    pub exposure_ev: Option<i32>,
    pub focus_mode: String,
    pub white_balance: String,
    pub fps: (i32, i32),
    pub resolution: (u32, u32),
    pub screen_brightness: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrConfig {
    pub error_correction: ErrorCorrectionLevel,
    pub payload_size_bytes: usize,
    pub module_size_px: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuningResult {
    pub camera_config_id: u32,
    pub qr_config: QrConfig,
    pub decode_rate: f32,
    pub avg_latency_ms: f32,
    pub jitter_ms: f32,
    pub thermal_events: u32,
    pub frames_total: u32,
    pub frames_decoded: u32,
    pub actual_iso: Option<i32>,
    pub actual_exposure_ev: Option<i32>,
}

pub struct SweepMatrix {
    pub camera_configs: Vec<CameraConfig>,
    pub qr_configs: Vec<QrConfig>,
}

/// Score a tuning result for ranking camera configurations.
///
/// Weights: decode_rate 50%, latency 30%, jitter 20%.
/// Thermal events incur a flat penalty of -0.1 each.
/// Latency and jitter are clamped to a minimum of 1.0 ms to avoid division by zero.
pub fn score_config(result: &TuningResult) -> f32 {
    let thermal_penalty = result.thermal_events as f32 * -0.1;
    (result.decode_rate * 0.50)
        + ((1.0 / result.avg_latency_ms.max(1.0)) * 300.0 * 0.30)
        + ((1.0 / result.jitter_ms.max(1.0)) * 30.0 * 0.20)
        + thermal_penalty
}

/// Rank camera configurations by score (highest first).
///
/// Returns a list of `(camera_config_id, score)` pairs sorted in descending order.
pub fn rank_configs(results: &[TuningResult]) -> Vec<(u32, f32)> {
    let mut scored: Vec<(u32, f32)> = results
        .iter()
        .map(|r| (r.camera_config_id, score_config(r)))
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored
}
