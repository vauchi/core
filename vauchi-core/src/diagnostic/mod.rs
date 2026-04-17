// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod debug_session;
#[cfg(feature = "testing")]
pub mod exchange_debug;
#[cfg(not(feature = "testing"))]
pub(crate) mod exchange_debug;
pub mod log_event;
#[cfg(feature = "diagnostic-scanner")]
pub mod preprocess;
pub mod report;
#[cfg(feature = "diagnostic-scanner")]
pub mod scanner;
pub mod snapshot;
pub mod tuner;

pub use debug_session::DebugSession;
pub use log_event::{LogEvent, LogEventKind, ScreenId};
pub use report::generate_html_report;
pub use snapshot::{BoundingBox, SnapshotMetadata};
pub use tuner::{
    CameraConfig, DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, SweepMatrix,
    ThroughputFrame, TuningResult, generate_extended_qr_test_patterns, generate_qr_test_patterns,
    generate_sweep_matrix, generate_throughput_sequence, rank_configs, score_config,
};
