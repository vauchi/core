// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

use super::tuner::QrConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundingBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    pub timestamp_ms: u64,
    pub config_id: u32,
    pub qr_config: QrConfig,
    pub frame_index: u32,
    pub decode_result: bool,
    pub decode_latency_ms: Option<f32>,
    pub bounding_box: Option<BoundingBox>,
    pub actual_iso: Option<i32>,
    pub actual_exposure_ev: Option<i32>,
    pub actual_focus_distance: Option<f32>,
    pub redacted: bool,
}
