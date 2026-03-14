// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Debug Session
//!
//! Zero-cost debug instrumentation wrapper. When inactive, all methods
//! are no-ops. When activated (via gesture, settings toggle, or launch
//! argument), captures timestamped UX events for diagnostic analysis.

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
    pub fn activate(&mut self) {
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
            .filter_map(|e| serde_json::to_string(e).ok())
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
    pub fn log_user_action(&mut self, screen: ScreenId, action: String) {
        self.push_event(LogEventKind::UserAction { screen, action });
    }

    /// Log a flow being abandoned.
    pub fn log_flow_abandoned(&mut self, screen: ScreenId, reason: String) {
        self.push_event(LogEventKind::FlowAbandoned { screen, reason });
    }

    /// Log a retry attempt.
    pub fn log_retry_attempted(&mut self, screen: ScreenId, attempt: u32) {
        self.push_event(LogEventKind::RetryAttempted { screen, attempt });
    }

    /// Log an error presented to the user.
    pub fn log_error_presented(&mut self, screen: ScreenId, error: String) {
        self.push_event(LogEventKind::ErrorPresented { screen, error });
    }

    /// Log a tester note.
    pub fn log_tester_note(&mut self, note: String) {
        self.push_event(LogEventKind::TesterNote { note });
    }

    /// Log a session marker for segmenting analysis.
    pub fn log_session_marker(&mut self, label: String) {
        self.push_event(LogEventKind::DebugSessionMarker { label });
    }

    // --- Internal ---

    fn push_event(&mut self, kind: LogEventKind) {
        if !self.active {
            return;
        }
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
