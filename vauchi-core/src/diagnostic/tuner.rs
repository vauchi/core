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

const DESIRED_ISO: &[i32] = &[50, 100, 200, 400, 800];
const DESIRED_EV: &[i32] = &[-2, -1, 0, 1, 2];
const DESIRED_BRIGHTNESS: &[f32] = &[0.5, 0.75, 1.0];
const DESIRED_PAYLOAD_SIZES: &[usize] = &[100, 250, 472];
const DESIRED_MODULE_SIZES: &[u32] = &[6, 10, 14];

fn filter_range(desired: &[i32], range: Option<(i32, i32)>) -> Vec<Option<i32>> {
    match range {
        Some((min, max)) => desired
            .iter()
            .filter(|&&v| v >= min && v <= max)
            .map(|&v| Some(v))
            .collect(),
        None => vec![None],
    }
}

/// Generate a sweep matrix of camera and QR configurations to test.
///
/// Produces a combinatorial set of [`CameraConfig`] values based on the device's
/// capability profile, and a fixed set of [`QrConfig`] values covering all error
/// correction levels, payload sizes, and module sizes.
pub fn generate_sweep_matrix(profile: &DeviceCapabilityProfile) -> SweepMatrix {
    let iso_values = filter_range(DESIRED_ISO, profile.iso_range);
    let ev_values = filter_range(DESIRED_EV, profile.exposure_ev_range);

    let focus_modes: Vec<String> = if profile.af_modes.is_empty() {
        vec!["auto".into()]
    } else {
        profile.af_modes.clone()
    };

    let wb_modes: Vec<String> = if profile.awb_modes.is_empty() {
        vec!["auto".into()]
    } else {
        profile.awb_modes.clone()
    };

    let fps_values: Vec<(i32, i32)> = if profile.fps_ranges.is_empty() {
        vec![(30, 30)]
    } else {
        profile.fps_ranges.clone()
    };

    let mut camera_configs = Vec::new();
    let mut id = 0u32;

    for iso in &iso_values {
        for ev in &ev_values {
            for focus in &focus_modes {
                for wb in &wb_modes {
                    for fps in &fps_values {
                        for &brightness in DESIRED_BRIGHTNESS {
                            camera_configs.push(CameraConfig {
                                id,
                                iso: *iso,
                                exposure_ev: *ev,
                                focus_mode: focus.clone(),
                                white_balance: wb.clone(),
                                fps: *fps,
                                resolution: profile.max_resolution,
                                screen_brightness: brightness,
                            });
                            id += 1;
                        }
                    }
                }
            }
        }
    }

    let mut qr_configs = Vec::new();
    for &ec in &[
        ErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H,
    ] {
        for &size in DESIRED_PAYLOAD_SIZES {
            for &module in DESIRED_MODULE_SIZES {
                qr_configs.push(QrConfig {
                    error_correction: ec,
                    payload_size_bytes: size,
                    module_size_px: module,
                });
            }
        }
    }

    SweepMatrix {
        camera_configs,
        qr_configs,
    }
}

/// Generate deterministic QR test patterns covering all error correction levels.
///
/// Returns a Vec of (QrConfig, payload) pairs across multiple payload sizes,
/// module sizes, and EC levels. Payloads are deterministic ASCII strings.
pub fn generate_qr_test_patterns() -> Vec<(QrConfig, String)> {
    let mut patterns = Vec::new();

    for &ec in &[
        ErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H,
    ] {
        for &size in DESIRED_PAYLOAD_SIZES {
            for &module in DESIRED_MODULE_SIZES {
                let config = QrConfig {
                    error_correction: ec,
                    payload_size_bytes: size,
                    module_size_px: module,
                };
                let pattern: String = (0..size)
                    .map(|i| {
                        let byte = ((i * 7 + 13) % 62) as u8;
                        match byte {
                            0..=9 => (b'0' + byte) as char,
                            10..=35 => (b'A' + byte - 10) as char,
                            36..=61 => (b'a' + byte - 36) as char,
                            _ => unreachable!(),
                        }
                    })
                    .collect();
                patterns.push((config, pattern));
            }
        }
    }

    patterns
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
