// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod debug_session;
#[cfg(feature = "testing")]
pub mod exchange_debug;
#[cfg(not(feature = "testing"))]
pub(crate) mod exchange_debug;
pub mod log_event;
pub mod report;
pub mod snapshot;
pub mod tuner;

pub use debug_session::DebugSession;
pub use log_event::{LogEvent, LogEventKind, ScreenId};
pub use report::generate_html_report;
pub use snapshot::{BoundingBox, SnapshotMetadata};
pub use tuner::{
    CameraConfig, DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, SweepMatrix,
    TuningResult, generate_qr_test_patterns, generate_sweep_matrix, rank_configs, score_config,
};
