// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod log_event;
pub mod snapshot;
pub mod tuner;

pub use log_event::{LogEvent, LogEventKind};
pub use snapshot::{BoundingBox, SnapshotMetadata};
pub use tuner::{
    generate_qr_test_patterns, generate_sweep_matrix, rank_configs, score_config, CameraConfig,
    DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, SweepMatrix, TuningResult,
};
