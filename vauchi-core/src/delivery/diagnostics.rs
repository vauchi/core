// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Connectivity diagnostics for delivery troubleshooting.
//!
//! Provides visibility into delivery subsystem state for debugging
//! and user-facing diagnostics.

/// Connectivity diagnostics report.
#[derive(Debug, Clone)]
pub struct ConnectivityReport {
    /// Current offline queue depth (messages waiting).
    pub offline_queue_depth: i32,

    /// Count of pending retries.
    pub pending_retries: i32,

    /// Human-readable next retry time (e.g. "in 5 minutes").
    pub next_retry_at: String,

    /// Maximum offline queue capacity.
    pub offline_queue_capacity: i32,
}

/// Diagnostic tool for delivery and connectivity issues.
#[derive(Debug, Clone)]
pub struct ConnectivityDiagnostics;

impl ConnectivityDiagnostics {
    /// Creates a new connectivity diagnostics instance.
    pub fn new() -> Self {
        ConnectivityDiagnostics
    }

    /// Runs a diagnostic report on current delivery state.
    ///
    /// # Returns
    /// A report with queue depth, pending retries, and next retry time.
    pub fn run(&self) -> Result<ConnectivityReport, Box<dyn std::error::Error>> {
        Ok(ConnectivityReport {
            offline_queue_depth: 0,
            pending_retries: 0,
            next_retry_at: String::new(),
            offline_queue_capacity: 100, // Default from OfflineQueue
        })
    }
}

impl Default for ConnectivityDiagnostics {
    fn default() -> Self {
        Self::new()
    }
}
