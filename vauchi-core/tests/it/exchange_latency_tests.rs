// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange latency measurement infrastructure.
//!
//! Verifies the `CommandDispatched` debug event and `latency_summary()`
//! computation on ExchangeDebugLog.

#![cfg(feature = "testing")]

use vauchi_core::diagnostic::exchange_debug::{
    ExchangeDebugEvent, ExchangeDebugLog, LatencySummary,
};

// ===== CommandDispatched event =====

// @internal
#[test]
fn command_dispatched_event_records_command_name() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::CommandDispatched {
        command_name: "QrDisplay".to_string(),
    });

    let events = log.events();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0].event,
        ExchangeDebugEvent::CommandDispatched { command_name } if command_name == "QrDisplay"
    ));
}

// @internal
#[test]
fn command_dispatched_serializes_to_jsonl() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::CommandDispatched {
        command_name: "BleStartScanning".to_string(),
    });

    let jsonl = log.to_jsonl();
    assert!(
        jsonl.contains("command_dispatched"),
        "JSONL should contain command_dispatched tag"
    );
    assert!(
        jsonl.contains("BleStartScanning"),
        "JSONL should contain command name"
    );
}

// @internal
#[test]
fn command_dispatched_appears_in_markdown() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::CommandDispatched {
        command_name: "NfcActivate".to_string(),
    });

    let md = log.to_markdown();
    assert!(md.contains("CommandDispatched"));
    assert!(md.contains("NfcActivate"));
}

// ===== Latency summary =====

// @internal
#[test]
fn latency_summary_empty_log_returns_none() {
    let log = ExchangeDebugLog::new();
    assert!(log.latency_summary().is_none());
}

// @internal
#[test]
fn latency_summary_computes_deltas_for_full_qr_flow() {
    let mut log = ExchangeDebugLog::new();

    // Simulate a full QR exchange timeline by pushing events with known order.
    // Since we can't control Instant timing in unit tests, we verify the
    // structure and that deltas are non-negative.
    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::KeyAgreementCompleted);
    log.push(ExchangeDebugEvent::ExchangeCompleted);

    let summary = log.latency_summary().expect("should have summary");

    // All deltas should be present for a full flow
    assert!(summary.session_to_qr_generated_ms.is_some());
    assert!(summary.qr_generated_to_scanned_ms.is_some());
    assert!(summary.qr_scanned_to_key_agreement_ms.is_some());
    assert!(summary.key_agreement_to_completed_ms.is_some());
    assert!(summary.total_ms.is_some());

    // Deltas must be non-negative (events pushed in rapid succession)
    assert!(summary.session_to_qr_generated_ms.unwrap() >= 0);
    assert!(summary.qr_generated_to_scanned_ms.unwrap() >= 0);
    assert!(summary.qr_scanned_to_key_agreement_ms.unwrap() >= 0);
    assert!(summary.key_agreement_to_completed_ms.unwrap() >= 0);
    assert!(summary.total_ms.unwrap() >= 0);
}

// @internal
#[test]
fn latency_summary_partial_flow_has_none_for_missing_segments() {
    let mut log = ExchangeDebugLog::new();

    // Only session start and QR generated — no scan, no key agreement
    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);

    let summary = log.latency_summary().expect("should have summary");

    assert!(summary.session_to_qr_generated_ms.is_some());
    assert!(summary.qr_generated_to_scanned_ms.is_none());
    assert!(summary.qr_scanned_to_key_agreement_ms.is_none());
    assert!(summary.key_agreement_to_completed_ms.is_none());
    assert!(summary.total_ms.is_none());
}

// @internal
#[test]
fn latency_summary_failed_exchange_has_no_completion() {
    let mut log = ExchangeDebugLog::new();

    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::ExchangeFailed {
        error: "timeout".to_string(),
    });

    let summary = log.latency_summary().expect("should have summary");

    assert!(summary.session_to_qr_generated_ms.is_some());
    assert!(summary.qr_generated_to_scanned_ms.is_some());
    // No key agreement or completion
    assert!(summary.qr_scanned_to_key_agreement_ms.is_none());
    assert!(summary.key_agreement_to_completed_ms.is_none());
    assert!(summary.total_ms.is_none());
}

// @internal
#[test]
fn latency_summary_serializes_to_json() {
    let mut log = ExchangeDebugLog::new();

    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::KeyAgreementCompleted);
    log.push(ExchangeDebugEvent::ExchangeCompleted);

    let summary = log.latency_summary().unwrap();
    let json = serde_json::to_string(&summary).expect("should serialize");

    assert!(json.contains("session_to_qr_generated_ms"));
    assert!(json.contains("qr_generated_to_scanned_ms"));
    assert!(json.contains("qr_scanned_to_key_agreement_ms"));
    assert!(json.contains("key_agreement_to_completed_ms"));
    assert!(json.contains("total_ms"));
}

// @internal
#[test]
fn latency_summary_total_equals_sum_of_segments() {
    let mut log = ExchangeDebugLog::new();

    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::KeyAgreementCompleted);
    log.push(ExchangeDebugEvent::ExchangeCompleted);

    let summary = log.latency_summary().unwrap();

    // Total should equal SessionStarted → ExchangeCompleted
    // which is the sum of all segments
    let segment_sum = summary.session_to_qr_generated_ms.unwrap()
        + summary.qr_generated_to_scanned_ms.unwrap()
        + summary.qr_scanned_to_key_agreement_ms.unwrap()
        + summary.key_agreement_to_completed_ms.unwrap();

    assert_eq!(summary.total_ms.unwrap(), segment_sum);
}

// ===== Session integration: CommandDispatched fires on emit_command =====

use vauchi_core::exchange::*;
use vauchi_core::*;

// @internal
#[test]
fn session_emits_command_dispatched_on_initial_commands() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let mut session = ExchangeSession::new_qr(
        identity,
        card,
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    session.enable_debug_log();

    session.apply(ExchangeEvent::StartQR).unwrap();
    session.emit_initial_commands();
    let _ = session.drain_commands();

    let log = session.exchange_debug_log().unwrap();
    let dispatched: Vec<_> = log
        .events()
        .iter()
        .filter(|e| matches!(&e.event, ExchangeDebugEvent::CommandDispatched { .. }))
        .collect();

    assert_eq!(dispatched.len(), 1);
    assert!(matches!(
        &dispatched[0].event,
        ExchangeDebugEvent::CommandDispatched { command_name } if command_name == "QrDisplay"
    ));
}

// @internal
#[test]
fn full_qr_session_produces_latency_summary() {
    let alice_identity = Identity::create("Alice");
    let bob_identity = Identity::create("Bob");

    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let bob_card = ContactCard::new("Bob");
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card.clone(),
        MockProximityVerifier::success(),
        vauchi_core::clock::SystemClock::shared(),
    );
    bob_session.enable_debug_log();

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();
    bob_session
        .apply(ExchangeEvent::CompleteExchange(alice_card))
        .unwrap();

    let log = bob_session.exchange_debug_log().unwrap();
    let summary = log
        .latency_summary()
        .expect("full flow should produce summary");

    // All segments present
    assert!(summary.session_to_qr_generated_ms.is_some());
    assert!(summary.qr_generated_to_scanned_ms.is_some());
    assert!(summary.qr_scanned_to_key_agreement_ms.is_some());
    assert!(summary.key_agreement_to_completed_ms.is_some());
    assert!(summary.total_ms.is_some());

    // Total is consistent with segments
    let segment_sum = summary.session_to_qr_generated_ms.unwrap()
        + summary.qr_generated_to_scanned_ms.unwrap()
        + summary.qr_scanned_to_key_agreement_ms.unwrap()
        + summary.key_agreement_to_completed_ms.unwrap();
    assert_eq!(summary.total_ms.unwrap(), segment_sum);

    // JSONL export includes all milestone events
    let jsonl = log.to_jsonl();
    assert!(jsonl.contains("session_started"));
    assert!(jsonl.contains("qr_generated"));
    assert!(jsonl.contains("qr_scanned"));
    assert!(jsonl.contains("key_agreement_completed"));
    assert!(jsonl.contains("exchange_completed"));
    assert!(jsonl.contains("command_dispatched"));
}
