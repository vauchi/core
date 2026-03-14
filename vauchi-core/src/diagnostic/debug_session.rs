// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug Session
//!
//! Zero-cost debug instrumentation wrapper. When inactive, all methods
//! are no-ops. When activated (via gesture, settings toggle, or launch
//! argument), captures timestamped UX events for diagnostic analysis.

use std::fmt::Write;
use std::time::Instant;

use super::log_event::{LogEvent, LogEventKind};

pub use super::log_event::ScreenId;

/// Debug instrumentation session.
///
/// Wraps event collection behind an activation guard. When inactive,
/// every logging method is a no-op with near-zero runtime cost (a single
/// branch on a bool). When activated, events are timestamped relative
/// to activation time and collected for later export.
pub struct DebugSession {
    active: bool,
    start: Instant,
    events: Vec<LogEvent>,
}

impl DebugSession {
    /// Create a new inactive debug session.
    pub fn new() -> Self {
        Self {
            active: false,
            start: Instant::now(),
            events: Vec::new(),
        }
    }

    /// Whether the debug session is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Activate the debug session. Logs a `DebugModeActivated` event.
    ///
    /// Idempotent: calling `activate()` on an already-active session is a no-op.
    /// This prevents timestamp discontinuities that would make the JSONL export
    /// unanalyzable.
    pub fn activate(&mut self) {
        if self.active {
            return;
        }
        self.active = true;
        self.start = Instant::now();
        self.push_event(LogEventKind::DebugModeActivated);
    }

    /// Deactivate the debug session. Events are preserved for export.
    pub fn deactivate(&mut self) {
        self.active = false;
    }

    /// Get all recorded events.
    pub fn events(&self) -> &[LogEvent] {
        &self.events
    }

    /// Export all events as JSONL (one JSON object per line).
    pub fn to_jsonl(&self) -> String {
        self.events
            .iter()
            .map(|e| serde_json::to_string(e).expect("LogEvent serialization cannot fail"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // --- UX event methods ---

    /// Log a screen appearing.
    pub fn log_screen_appeared(&mut self, screen: ScreenId) {
        self.push_event(LogEventKind::ScreenAppeared { screen });
    }

    /// Log a screen being dismissed.
    pub fn log_screen_dismissed(&mut self, screen: ScreenId) {
        self.push_event(LogEventKind::ScreenDismissed { screen });
    }

    /// Log a user action on a screen.
    ///
    /// # Privacy
    /// Callers must not include PII (contact names, keys, card content) in `action`.
    pub fn log_user_action(&mut self, screen: ScreenId, action: String) {
        self.push_event(LogEventKind::UserAction { screen, action });
    }

    /// Log a flow being abandoned.
    ///
    /// # Privacy
    /// Callers must not include PII (contact names, keys, card content) in `reason`.
    pub fn log_flow_abandoned(&mut self, screen: ScreenId, reason: String) {
        self.push_event(LogEventKind::FlowAbandoned { screen, reason });
    }

    /// Log a retry attempt.
    pub fn log_retry_attempted(&mut self, screen: ScreenId, attempt: u32) {
        self.push_event(LogEventKind::RetryAttempted { screen, attempt });
    }

    /// Log an error presented to the user.
    ///
    /// # Privacy
    /// Callers must not include PII (contact names, keys, card content) in `error`.
    pub fn log_error_presented(&mut self, screen: ScreenId, error: String) {
        self.push_event(LogEventKind::ErrorPresented { screen, error });
    }

    /// Log a tester note.
    ///
    /// # Privacy
    /// Callers must not include PII (contact names, keys, card content) in `note`.
    pub fn log_tester_note(&mut self, note: String) {
        self.push_event(LogEventKind::TesterNote { note });
    }

    /// Log a session marker for segmenting analysis.
    pub fn log_session_marker(&mut self, label: String) {
        self.push_event(LogEventKind::DebugSessionMarker { label });
    }

    /// Export session as a human-readable Markdown report.
    pub fn to_markdown(&self) -> String {
        let status = if self.active { "active" } else { "inactive" };
        let count = self.events.len();
        let mut md = String::new();
        writeln!(md, "# Debug Session Report\n").unwrap();
        writeln!(md, "**Status:** {status} | **{count} events**\n").unwrap();

        if self.events.is_empty() {
            writeln!(md, "_No events recorded._").unwrap();
            return md;
        }

        writeln!(md, "| Timestamp (ms) | Event |").unwrap();
        writeln!(md, "|---:|---|").unwrap();
        for event in &self.events {
            let desc = Self::event_description(&event.kind);
            let safe = desc
                .replace('|', "\\|")
                .replace('\n', " ")
                .replace('\r', "");
            writeln!(md, "| {} | {} |", event.timestamp_ms, safe).unwrap();
        }
        md
    }

    /// Human-readable description of a log event kind.
    fn event_description(kind: &LogEventKind) -> String {
        match kind {
            LogEventKind::DebugModeActivated => "DebugModeActivated".to_string(),
            LogEventKind::ScreenAppeared { screen } => {
                format!("ScreenAppeared — {screen:?}")
            }
            LogEventKind::ScreenDismissed { screen } => {
                format!("ScreenDismissed — {screen:?}")
            }
            LogEventKind::UserAction { screen, action } => {
                format!("UserAction — {screen:?}: {action}")
            }
            LogEventKind::FlowAbandoned { screen, reason } => {
                format!("FlowAbandoned — {screen:?}: {reason}")
            }
            LogEventKind::RetryAttempted { screen, attempt } => {
                format!("RetryAttempted — {screen:?} #{attempt}")
            }
            LogEventKind::ErrorPresented { screen, error } => {
                format!("ErrorPresented — {screen:?}: {error}")
            }
            LogEventKind::TesterNote { note } => format!("TesterNote — {note}"),
            LogEventKind::DebugSessionMarker { label } => {
                format!("SessionMarker — {label}")
            }
            // Camera/QR tuner events — controlled output, no raw paths
            LogEventKind::DecodeSuccess {
                latency_ms,
                frame_index,
            } => format!("DecodeSuccess — frame {frame_index}, {latency_ms:.1}ms"),
            LogEventKind::DecodeFailure {
                reason,
                frame_index,
            } => format!("DecodeFailure — frame {frame_index}: {reason}"),
            LogEventKind::CameraConfigApplied { config_id, .. } => {
                format!("CameraConfigApplied — config {config_id}")
            }
            LogEventKind::CameraConfigFailed { config_id, reason } => {
                format!("CameraConfigFailed — config {config_id}: {reason}")
            }
            LogEventKind::ThermalState { state, temp_c } => {
                format!("ThermalState — {state} ({temp_c:.1}C)")
            }
            LogEventKind::SweepStarted { total_configs } => {
                format!("SweepStarted — {total_configs} configs")
            }
            LogEventKind::SweepPhaseComplete { phase, .. } => {
                format!("SweepPhaseComplete — phase {phase}")
            }
            LogEventKind::SweepComplete {
                best_config_id,
                best_score,
            } => format!("SweepComplete — config {best_config_id} (score {best_score:.2})"),
            LogEventKind::SnapshotSaved { frame_index, .. } => {
                // Omit path to avoid leaking filesystem layout
                format!("SnapshotSaved — frame {frame_index}")
            }
        }
    }

    // --- Internal ---

    fn push_event(&mut self, kind: LogEventKind) {
        if !self.active {
            return;
        }
        // as_millis() returns u128; truncation safe — overflows after ~585M years.
        let timestamp_ms = self.start.elapsed().as_millis() as u64;
        self.events.push(LogEvent {
            timestamp_ms,
            device_model: String::new(),
            kind,
        });
    }
}

impl Default for DebugSession {
    fn default() -> Self {
        Self::new()
    }
}
