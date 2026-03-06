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
