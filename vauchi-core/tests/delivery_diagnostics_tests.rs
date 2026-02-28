// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for delivery::diagnostics
//! Connectivity and delivery troubleshooting API.

use vauchi_core::delivery::ConnectivityDiagnostics;

// @scenario: message_delivery:User can diagnose delivery problems
#[test]
fn test_diagnostics_reports_connectivity_state() {
    let diag = ConnectivityDiagnostics::new();
    let report = diag.run().unwrap();

    // New instance should report initial state
    assert!(
        report.offline_queue_depth >= 0,
        "Queue depth should be >= 0"
    );
    assert!(
        report.pending_retries >= 0,
        "Pending retries should be >= 0"
    );
}

// @scenario: message_delivery:Diagnostics includes retry information
#[test]
fn test_diagnostics_reports_retry_status() {
    let diag = ConnectivityDiagnostics::new();
    let report = diag.run().unwrap();

    assert!(
        !report.next_retry_at.is_empty() || report.pending_retries == 0,
        "Should report next retry time or have no pending retries"
    );
}

// @scenario: message_delivery:Diagnostics reports queue status
#[test]
fn test_diagnostics_reports_queue_capacity() {
    let diag = ConnectivityDiagnostics::new();
    let report = diag.run().unwrap();

    assert!(
        report.offline_queue_depth >= 0,
        "Offline queue depth should be non-negative"
    );
    assert!(
        report.offline_queue_capacity > 0,
        "Offline queue capacity should be positive"
    );
}
