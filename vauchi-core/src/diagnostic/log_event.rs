// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum LogEventKind {
    DecodeSuccess {
        latency_ms: f32,
        frame_index: u32,
    },
    DecodeFailure {
        reason: String,
        frame_index: u32,
    },
    CameraConfigApplied {
        config_id: u32,
        iso: i32,
        ev: i32,
        fps: i32,
    },
    CameraConfigFailed {
        config_id: u32,
        reason: String,
    },
    ThermalState {
        state: String,
        temp_c: f32,
    },
    SweepStarted {
        total_configs: u32,
    },
    SweepPhaseComplete {
        phase: u32,
        top_configs: Vec<u32>,
    },
    SweepComplete {
        best_config_id: u32,
        best_score: f32,
    },
    SnapshotSaved {
        frame_index: u32,
        path: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp_ms: u64,
    pub device_model: String,
    pub kind: LogEventKind,
}
