// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the diagnostic tuner module.
//!
//! Wraps core diagnostic types, flattening tuples and converting `usize`
//! to `u32` for UniFFI compatibility.

use vauchi_core::diagnostic::{
    DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, TuningResult,
    generate_qr_test_patterns, generate_sweep_matrix, rank_configs, score_config,
};

// === Mobile Wrapper Types ===

#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobilePlatform {
    Android,
    Ios,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileErrorCorrectionLevel {
    L,
    M,
    Q,
    H,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileFpsRange {
    pub min: i32,
    pub max: i32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileDeviceCapabilityProfile {
    pub platform: MobilePlatform,
    pub device_model: String,
    pub hardware_level: Option<String>,
    pub iso_range_min: Option<i32>,
    pub iso_range_max: Option<i32>,
    pub exposure_ev_range_min: Option<i32>,
    pub exposure_ev_range_max: Option<i32>,
    pub af_modes: Vec<String>,
    pub awb_modes: Vec<String>,
    pub fps_ranges: Vec<MobileFpsRange>,
    pub max_resolution_width: u32,
    pub max_resolution_height: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileCameraConfig {
    pub id: u32,
    pub iso: Option<i32>,
    pub exposure_ev: Option<i32>,
    pub focus_mode: String,
    pub white_balance: String,
    pub fps_min: i32,
    pub fps_max: i32,
    pub width: u32,
    pub height: u32,
    pub screen_brightness: f32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileQrConfig {
    pub error_correction: MobileErrorCorrectionLevel,
    pub payload_size_bytes: u32,
    pub module_size_px: u32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileTuningResult {
    pub camera_config_id: u32,
    pub qr_error_correction: MobileErrorCorrectionLevel,
    pub qr_payload_size_bytes: u32,
    pub qr_module_size_px: u32,
    pub decode_rate: f32,
    pub avg_latency_ms: f32,
    pub jitter_ms: f32,
    pub thermal_events: u32,
    pub frames_total: u32,
    pub frames_decoded: u32,
    pub actual_iso: Option<i32>,
    pub actual_exposure_ev: Option<i32>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileScoredConfig {
    pub config_id: u32,
    pub score: f32,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileSweepMatrix {
    pub camera_configs: Vec<MobileCameraConfig>,
    pub qr_configs: Vec<MobileQrConfig>,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileQrTestPattern {
    pub config: MobileQrConfig,
    pub data: String,
}

// === Conversions: Mobile -> Core ===

impl From<&MobilePlatform> for Platform {
    fn from(p: &MobilePlatform) -> Self {
        match p {
            MobilePlatform::Android => Platform::Android,
            MobilePlatform::Ios => Platform::Ios,
        }
    }
}

impl From<&MobileErrorCorrectionLevel> for ErrorCorrectionLevel {
    fn from(ec: &MobileErrorCorrectionLevel) -> Self {
        match ec {
            MobileErrorCorrectionLevel::L => ErrorCorrectionLevel::L,
            MobileErrorCorrectionLevel::M => ErrorCorrectionLevel::M,
            MobileErrorCorrectionLevel::Q => ErrorCorrectionLevel::Q,
            MobileErrorCorrectionLevel::H => ErrorCorrectionLevel::H,
        }
    }
}

impl From<&MobileDeviceCapabilityProfile> for DeviceCapabilityProfile {
    fn from(p: &MobileDeviceCapabilityProfile) -> Self {
        let iso_range = match (p.iso_range_min, p.iso_range_max) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => None,
        };
        let exposure_ev_range = match (p.exposure_ev_range_min, p.exposure_ev_range_max) {
            (Some(min), Some(max)) => Some((min, max)),
            _ => None,
        };
        DeviceCapabilityProfile {
            platform: Platform::from(&p.platform),
            device_model: p.device_model.clone(),
            hardware_level: p.hardware_level.clone(),
            iso_range,
            exposure_ev_range,
            af_modes: p.af_modes.clone(),
            awb_modes: p.awb_modes.clone(),
            fps_ranges: p.fps_ranges.iter().map(|r| (r.min, r.max)).collect(),
            max_resolution: (p.max_resolution_width, p.max_resolution_height),
        }
    }
}

impl From<&MobileTuningResult> for TuningResult {
    fn from(r: &MobileTuningResult) -> Self {
        TuningResult {
            camera_config_id: r.camera_config_id,
            qr_config: QrConfig {
                error_correction: ErrorCorrectionLevel::from(&r.qr_error_correction),
                payload_size_bytes: r.qr_payload_size_bytes as usize,
                module_size_px: r.qr_module_size_px,
            },
            decode_rate: r.decode_rate,
            avg_latency_ms: r.avg_latency_ms,
            jitter_ms: r.jitter_ms,
            thermal_events: r.thermal_events,
            frames_total: r.frames_total,
            frames_decoded: r.frames_decoded,
            actual_iso: r.actual_iso,
            actual_exposure_ev: r.actual_exposure_ev,
        }
    }
}

// === Conversions: Core -> Mobile ===

fn ec_to_mobile(ec: &ErrorCorrectionLevel) -> MobileErrorCorrectionLevel {
    match ec {
        ErrorCorrectionLevel::L => MobileErrorCorrectionLevel::L,
        ErrorCorrectionLevel::M => MobileErrorCorrectionLevel::M,
        ErrorCorrectionLevel::Q => MobileErrorCorrectionLevel::Q,
        ErrorCorrectionLevel::H => MobileErrorCorrectionLevel::H,
        _ => MobileErrorCorrectionLevel::M,
    }
}

fn camera_config_to_mobile(c: &vauchi_core::diagnostic::CameraConfig) -> MobileCameraConfig {
    MobileCameraConfig {
        id: c.id,
        iso: c.iso,
        exposure_ev: c.exposure_ev,
        focus_mode: c.focus_mode.clone(),
        white_balance: c.white_balance.clone(),
        fps_min: c.fps.0,
        fps_max: c.fps.1,
        width: c.resolution.0,
        height: c.resolution.1,
        screen_brightness: c.screen_brightness,
    }
}

fn qr_config_to_mobile(q: &QrConfig) -> MobileQrConfig {
    MobileQrConfig {
        error_correction: ec_to_mobile(&q.error_correction),
        payload_size_bytes: q.payload_size_bytes as u32,
        module_size_px: q.module_size_px,
    }
}

// === Exported Functions ===

#[uniffi::export]
pub fn diagnostic_generate_sweep_matrix(
    profile: MobileDeviceCapabilityProfile,
) -> MobileSweepMatrix {
    let core_profile = DeviceCapabilityProfile::from(&profile);
    let matrix = generate_sweep_matrix(&core_profile);
    MobileSweepMatrix {
        camera_configs: matrix
            .camera_configs
            .iter()
            .map(camera_config_to_mobile)
            .collect(),
        qr_configs: matrix.qr_configs.iter().map(qr_config_to_mobile).collect(),
    }
}

#[uniffi::export]
pub fn diagnostic_score_config(result: MobileTuningResult) -> f32 {
    let core_result = TuningResult::from(&result);
    score_config(&core_result)
}

#[uniffi::export]
pub fn diagnostic_rank_configs(results: Vec<MobileTuningResult>) -> Vec<MobileScoredConfig> {
    let core_results: Vec<TuningResult> = results.iter().map(TuningResult::from).collect();
    rank_configs(&core_results)
        .into_iter()
        .map(|(config_id, score)| MobileScoredConfig { config_id, score })
        .collect()
}

#[uniffi::export]
pub fn diagnostic_generate_qr_test_patterns() -> Vec<MobileQrTestPattern> {
    generate_qr_test_patterns()
        .into_iter()
        .map(|(config, data)| MobileQrTestPattern {
            config: qr_config_to_mobile(&config),
            data,
        })
        .collect()
}
