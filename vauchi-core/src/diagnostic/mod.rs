// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

// Production modules (always compiled):
// - scanner: rxing/rqrr QR decoding used by Android/iOS mobile scanner
// - exchange_debug: timestamped event log consumed by exchange/session.rs
#[cfg(feature = "testing")]
pub mod exchange_debug;
#[cfg(not(feature = "testing"))]
pub(crate) mod exchange_debug;
pub mod scanner;

// Development/benchmark modules (gated behind diagnostic-scanner):
// tuner, report, snapshot, debug_session, log_event, preprocess, yolo_detector.
// All are used only by the QR scanner benchmark harness (QrTuner UI,
// QR throughput tester, camera config sweeper, device profiling reports).
// Must never ship in production binaries.
#[cfg(feature = "diagnostic-scanner")]
pub mod debug_session;
#[cfg(feature = "diagnostic-scanner")]
pub mod log_event;
#[cfg(feature = "diagnostic-scanner")]
pub mod preprocess;
#[cfg(feature = "diagnostic-scanner")]
pub mod report;
#[cfg(feature = "diagnostic-scanner")]
pub mod snapshot;
#[cfg(feature = "diagnostic-scanner")]
pub mod tuner;
#[cfg(feature = "diagnostic-yolo")]
pub mod yolo_detector;

#[cfg(feature = "diagnostic-scanner")]
pub use debug_session::DebugSession;
#[cfg(feature = "diagnostic-scanner")]
pub use log_event::{LogEvent, LogEventKind, ScreenId};
#[cfg(feature = "diagnostic-scanner")]
pub use report::{
    BackendBenchmark, ThroughputBenchmark, generate_comparison_report, generate_html_report,
};
#[cfg(feature = "diagnostic-scanner")]
pub use snapshot::{BoundingBox, SnapshotMetadata};
#[cfg(feature = "diagnostic-scanner")]
pub use tuner::{
    CameraConfig, DeviceCapabilityProfile, ErrorCorrectionLevel, Platform, QrConfig, SweepMatrix,
    ThroughputFrame, TuningResult, generate_extended_qr_test_patterns, generate_qr_test_patterns,
    generate_sweep_matrix, generate_throughput_sequence, rank_configs, score_config,
};
