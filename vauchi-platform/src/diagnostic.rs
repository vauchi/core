// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the diagnostic tuner module.
//!
//! Wraps core diagnostic types, flattening tuples and converting `usize`
//! to `u32` for UniFFI compatibility.

use vauchi_core::diagnostic::{
    DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, TuningResult,
    generate_extended_qr_test_patterns, generate_qr_test_patterns, generate_sweep_matrix,
    generate_throughput_sequence, rank_configs, score_config,
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

#[uniffi::export]
pub fn diagnostic_generate_extended_qr_test_patterns() -> Vec<MobileQrTestPattern> {
    generate_extended_qr_test_patterns()
        .into_iter()
        .map(|(config, data)| MobileQrTestPattern {
            config: qr_config_to_mobile(&config),
            data,
        })
        .collect()
}

// === Throughput Sequence ===

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileThroughputFrame {
    pub frame_index: u32,
    pub total_frames: u32,
    pub data: String,
}

#[uniffi::export]
pub fn diagnostic_generate_throughput_sequence(
    total_bytes: u32,
    frame_capacity: u32,
) -> Vec<MobileThroughputFrame> {
    generate_throughput_sequence(total_bytes as usize, frame_capacity as usize)
        .into_iter()
        .map(|f| MobileThroughputFrame {
            frame_index: f.frame_index,
            total_frames: f.total_frames,
            data: f.data,
        })
        .collect()
}

// === QR Code Generation — replaces ZXing/ML Kit on mobile ===

/// Error correction level for QR generation.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileQrEccLevel {
    Low,
    Medium,
    Quartile,
    High,
}

/// Pre-rendered QR code bitmap (grayscale, 8-bit).
///
/// Frontends wrap this directly into a native image (UIImage, NSImage,
/// Bitmap) — no pixel loops needed on the frontend side.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileQrBitmap {
    /// Bitmap width in pixels (= height, always square).
    pub size: u32,
    /// Row-major grayscale pixels, length = `size × size`.
    /// 0 = dark (black), 255 = light (white), or custom values.
    pub pixels: Vec<u8>,
}

/// Generate a ready-to-display QR code bitmap with quiet zone and scaling.
///
/// Renders the QR modules into a grayscale pixel buffer at the requested
/// size. Frontends wrap the result into a native image with zero pixel
/// arithmetic — all rendering happens in Rust.
///
/// - `data`: the string to encode
/// - `size`: output bitmap width/height in pixels
/// - `ecc`: error correction level
/// - `dark`: grayscale value for dark modules (0 = black)
/// - `light`: grayscale value for light modules (255 = white)
/// - `margin`: quiet zone in modules (standard: 4)
#[uniffi::export]
pub fn generate_qr_bitmap(
    data: String,
    size: u32,
    ecc: MobileQrEccLevel,
    dark: u8,
    light: u8,
    margin: u32,
) -> Result<MobileQrBitmap, crate::error::MobileError> {
    use qrcode::{EcLevel, QrCode};

    let ec = match ecc {
        MobileQrEccLevel::Low => EcLevel::L,
        MobileQrEccLevel::Medium => EcLevel::M,
        MobileQrEccLevel::Quartile => EcLevel::Q,
        MobileQrEccLevel::High => EcLevel::H,
    };

    let code = QrCode::with_error_correction_level(data.as_bytes(), ec).map_err(|e| {
        crate::error::MobileError::ExchangeFailed(format!("QR generation failed: {e}"))
    })?;

    let qr_width = code.width() as u32;
    let total_modules = qr_width + 2 * margin;
    let size_px = size;
    let scale = size_px as f32 / total_modules as f32;

    // Pre-fill with light background
    let mut pixels = vec![light; (size_px * size_px) as usize];

    // Paint dark modules
    let colors = code.to_colors();
    for (i, color) in colors.iter().enumerate() {
        if *color == qrcode::Color::Dark {
            let qx = (i as u32) % qr_width;
            let qy = (i as u32) / qr_width;

            let px0 = ((qx + margin) as f32 * scale) as u32;
            let py0 = ((qy + margin) as f32 * scale) as u32;
            let px1 = (((qx + margin + 1) as f32 * scale) as u32).min(size_px);
            let py1 = (((qy + margin + 1) as f32 * scale) as u32).min(size_px);

            for py in py0..py1 {
                let row_start = (py * size_px + px0) as usize;
                let row_end = (py * size_px + px1) as usize;
                pixels[row_start..row_end].fill(dark);
            }
        }
    }

    Ok(MobileQrBitmap {
        size: size_px,
        pixels,
    })
}

// === Scanner Backend — always available (rxing/rqrr are non-optional) ===

#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileScannerBackend {
    RqrrRaw,
    RqrrPreprocessed,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileScanResult {
    pub decoded: Option<String>,
    pub total_us: u64,
    pub preprocessing_us: u64,
    pub decode_us: u64,
    pub frame_skipped: bool,
    pub laplacian_variance: f32,
}

#[cfg(feature = "diagnostic-scanner")]
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobilePreprocessConfig {
    pub target_width: u32,
    pub clahe_clip_limit: f32,
    pub clahe_tile_size: u32,
    pub threshold_window: u32,
    pub unsharp_sigma: f32,
    pub unsharp_amount: f32,
    pub sharpness_threshold: f32,
    pub apply_clahe: bool,
    pub apply_unsharp: bool,
    pub apply_threshold: bool,
}

#[uniffi::export]
pub fn diagnostic_scan_qr(
    backend: MobileScannerBackend,
    luma_data: Vec<u8>,
    width: u32,
    height: u32,
) -> MobileScanResult {
    use vauchi_core::diagnostic::scanner::{ScannerBackend, scan_qr_from_luma};

    let rust_backend = match backend {
        MobileScannerBackend::RqrrRaw => ScannerBackend::RqrrRaw,
        MobileScannerBackend::RqrrPreprocessed => ScannerBackend::RqrrPreprocessed,
    };
    let result = scan_qr_from_luma(rust_backend, &luma_data, width, height);
    MobileScanResult {
        decoded: result.decoded,
        total_us: result.total_us,
        preprocessing_us: result.preprocessing_us,
        decode_us: result.decode_us,
        frame_skipped: result.frame_skipped,
        laplacian_variance: result.laplacian_variance,
    }
}

#[cfg(feature = "diagnostic-scanner")]
#[uniffi::export]
pub fn diagnostic_scan_qr_with_config(
    backend: MobileScannerBackend,
    luma_data: Vec<u8>,
    width: u32,
    height: u32,
    config: MobilePreprocessConfig,
) -> MobileScanResult {
    use vauchi_core::diagnostic::preprocess::PreprocessConfig;
    use vauchi_core::diagnostic::scanner::{ScannerBackend, scan_qr_from_luma_with_config};

    let rust_backend = match backend {
        MobileScannerBackend::RqrrRaw => ScannerBackend::RqrrRaw,
        MobileScannerBackend::RqrrPreprocessed => ScannerBackend::RqrrPreprocessed,
    };
    let rust_config = PreprocessConfig {
        target_width: config.target_width,
        clahe_clip_limit: config.clahe_clip_limit,
        clahe_tile_size: config.clahe_tile_size,
        threshold_window: config.threshold_window,
        unsharp_sigma: config.unsharp_sigma,
        unsharp_amount: config.unsharp_amount,
        sharpness_threshold: config.sharpness_threshold,
        apply_clahe: config.apply_clahe,
        apply_unsharp: config.apply_unsharp,
        apply_threshold: config.apply_threshold,
    };
    let result =
        scan_qr_from_luma_with_config(rust_backend, &luma_data, width, height, &rust_config);
    MobileScanResult {
        decoded: result.decoded,
        total_us: result.total_us,
        preprocessing_us: result.preprocessing_us,
        decode_us: result.decode_us,
        frame_skipped: result.frame_skipped,
        laplacian_variance: result.laplacian_variance,
    }
}

// === YOLO Scanner (gated behind diagnostic-yolo) ===

#[cfg(feature = "diagnostic-yolo")]
use std::sync::Mutex;

#[cfg(feature = "diagnostic-yolo")]
static YOLO_DETECTOR: Mutex<Option<vauchi_core::diagnostic::yolo_detector::YoloDetector>> =
    Mutex::new(None);

/// Load the YOLO QR detector model from a file path.
/// Must be called once before `diagnostic_scan_qr_yolo`.
#[cfg(feature = "diagnostic-yolo")]
#[uniffi::export]
pub fn diagnostic_load_yolo_model(model_path: String) -> bool {
    use vauchi_core::diagnostic::yolo_detector::YoloDetector;
    match YoloDetector::load(std::path::Path::new(&model_path)) {
        Ok(det) => {
            *YOLO_DETECTOR.lock().unwrap() = Some(det);
            true
        }
        Err(_) => false,
    }
}

/// Scan a QR code using YOLO detection → crop → rqrr decode.
#[cfg(feature = "diagnostic-yolo")]
#[uniffi::export]
pub fn diagnostic_scan_qr_yolo(
    luma_data: Vec<u8>,
    width: u32,
    height: u32,
    confidence_threshold: f32,
) -> MobileScanResult {
    use vauchi_core::diagnostic::scanner::scan_qr_yolo;
    let mut guard = YOLO_DETECTOR.lock().unwrap();
    match guard.as_mut() {
        Some(detector) => {
            let result = scan_qr_yolo(detector, &luma_data, width, height, confidence_threshold);
            MobileScanResult {
                decoded: result.decoded,
                total_us: result.total_us,
                preprocessing_us: result.preprocessing_us,
                decode_us: result.decode_us,
                frame_skipped: result.frame_skipped,
                laplacian_variance: result.laplacian_variance,
            }
        }
        None => MobileScanResult {
            decoded: None,
            total_us: 0,
            preprocessing_us: 0,
            decode_us: 0,
            frame_skipped: false,
            laplacian_variance: 0.0,
        },
    }
}
