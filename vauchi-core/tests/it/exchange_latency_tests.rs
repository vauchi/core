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

// @internal — exact deltas: each segment must be the difference of adjacent
// milestone timestamps. `push_at` pins a known timeline so the computation
// itself is verified, not merely that segments are present.
#[test]
fn latency_summary_computes_exact_deltas_for_full_qr_flow() {
    let mut log = ExchangeDebugLog::new();

    log.push_at(
        0,
        ExchangeDebugEvent::SessionStarted {
            transport: "qr".to_string(),
        },
    );
    log.push_at(10, ExchangeDebugEvent::QrGenerated);
    log.push_at(30, ExchangeDebugEvent::QrScanned);
    log.push_at(60, ExchangeDebugEvent::KeyAgreementCompleted);
    log.push_at(100, ExchangeDebugEvent::ExchangeCompleted);

    let summary = log.latency_summary().expect("full flow produces a summary");

    assert_eq!(summary.session_to_qr_generated_ms, Some(10));
    assert_eq!(summary.qr_generated_to_scanned_ms, Some(20));
    assert_eq!(summary.qr_scanned_to_key_agreement_ms, Some(30));
    assert_eq!(summary.key_agreement_to_completed_ms, Some(40));
    assert_eq!(summary.total_ms, Some(100));
}

// @internal — a segment is computed only when both its endpoints occurred;
// missing milestones leave later segments (and the total) as None.
#[test]
fn latency_summary_partial_flow_computes_present_and_omits_missing() {
    let mut log = ExchangeDebugLog::new();

    // Only session start and QR generated — no scan, no key agreement.
    log.push_at(
        0,
        ExchangeDebugEvent::SessionStarted {
            transport: "qr".to_string(),
        },
    );
    log.push_at(15, ExchangeDebugEvent::QrGenerated);

    let summary = log.latency_summary().expect("summary from a partial flow");

    assert_eq!(summary.session_to_qr_generated_ms, Some(15));
    assert_eq!(summary.qr_generated_to_scanned_ms, None);
    assert_eq!(summary.qr_scanned_to_key_agreement_ms, None);
    assert_eq!(summary.key_agreement_to_completed_ms, None);
    assert_eq!(summary.total_ms, None);
}

// @internal — a failed exchange computes segments up to the failure point
// and leaves key-agreement/completion (and the total) as None.
#[test]
fn latency_summary_failed_exchange_computes_up_to_failure_only() {
    let mut log = ExchangeDebugLog::new();

    log.push_at(
        0,
        ExchangeDebugEvent::SessionStarted {
            transport: "qr".to_string(),
        },
    );
    log.push_at(12, ExchangeDebugEvent::QrGenerated);
    log.push_at(50, ExchangeDebugEvent::QrScanned);
    log.push_at(
        70,
        ExchangeDebugEvent::ExchangeFailed {
            error: "timeout".to_string(),
        },
    );

    let summary = log.latency_summary().expect("summary even on failure");

    assert_eq!(summary.session_to_qr_generated_ms, Some(12));
    assert_eq!(summary.qr_generated_to_scanned_ms, Some(38));
    assert_eq!(summary.qr_scanned_to_key_agreement_ms, None);
    assert_eq!(summary.key_agreement_to_completed_ms, None);
    assert_eq!(summary.total_ms, None);
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

// @internal — the total (SessionStarted → ExchangeCompleted) must equal the
// sum of the individual segments, with non-uniform gaps to catch off-by-one
// segment boundaries.
#[test]
fn latency_summary_total_equals_sum_of_segments() {
    let mut log = ExchangeDebugLog::new();

    log.push_at(
        0,
        ExchangeDebugEvent::SessionStarted {
            transport: "qr".to_string(),
        },
    );
    log.push_at(7, ExchangeDebugEvent::QrGenerated);
    log.push_at(19, ExchangeDebugEvent::QrScanned);
    log.push_at(44, ExchangeDebugEvent::KeyAgreementCompleted);
    log.push_at(90, ExchangeDebugEvent::ExchangeCompleted);

    let summary = log.latency_summary().unwrap();

    let segment_sum = summary.session_to_qr_generated_ms.unwrap()
        + summary.qr_generated_to_scanned_ms.unwrap()
        + summary.qr_scanned_to_key_agreement_ms.unwrap()
        + summary.key_agreement_to_completed_ms.unwrap();

    assert_eq!(segment_sum, 90);
    assert_eq!(summary.total_ms, Some(90));
}

// ===== Session integration: CommandDispatched fires on emit_command =====

use vauchi_core::exchange::*;
use vauchi_core::*;

// @internal
#[test]
fn session_emits_command_dispatched_on_initial_commands() {
    let identity = Identity::create("Alice", 0);
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
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);

    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();
    let alice_qr = ExchangeQR::generate(
        &alice_identity,
        &alice_ephemeral,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    );

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
