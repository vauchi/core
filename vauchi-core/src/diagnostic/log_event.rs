// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::{Deserialize, Serialize};

/// Cross-platform screen identifier for UX instrumentation.
///
/// Used by `DebugSession` to track screen transitions, user actions,
/// flow abandonment, and error presentation across all platforms.
/// Each variant maps to a logical screen, not a platform-specific view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScreenId {
    Onboarding,
    Home,
    ExchangeStart,
    ExchangeQrDisplay,
    ExchangeQrScan,
    ExchangeProximityVerification,
    ExchangeConfirmation,
    ExchangeSuccess,
    ExchangeFailure,
    ContactList,
    ContactDetail,
    SyncStatus,
    SyncConflictResolution,
    LinkDeviceStart,
    LinkDeviceQrDisplay,
    LinkDeviceQrScan,
    LinkDeviceConfirmation,
    LinkDeviceSuccess,
    Settings,
    DebugPanel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
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
        #[serde(skip_serializing, default)]
        path: String,
    },

    /// A screen became visible.
    ScreenAppeared {
        screen: ScreenId,
    },
    /// A screen was dismissed or navigated away from.
    ScreenDismissed {
        screen: ScreenId,
    },
    /// A user action occurred on a screen.
    UserAction {
        screen: ScreenId,
        action: String,
    },
    /// A flow was abandoned before completion.
    FlowAbandoned {
        screen: ScreenId,
        reason: String,
    },
    /// A retry was attempted.
    RetryAttempted {
        screen: ScreenId,
        attempt: u32,
    },
    /// An error was presented to the user.
    ErrorPresented {
        screen: ScreenId,
        error: String,
    },
    /// A tester-entered note for session annotation.
    TesterNote {
        note: String,
    },
    /// Debug mode was activated.
    DebugModeActivated,
    /// A session marker (for segmenting log analysis).
    DebugSessionMarker {
        label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEvent {
    pub timestamp_ms: u64,
    /// Device model identifier. Empty for non-camera debug events.
    #[serde(default)]
    pub device_model: String,
    pub kind: LogEventKind,
}
