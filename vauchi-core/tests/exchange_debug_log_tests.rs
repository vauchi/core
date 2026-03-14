// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ExchangeDebugLog — timestamped exchange flow event collection.

#![cfg(feature = "testing")]

use vauchi_core::diagnostic::exchange_debug::{ExchangeDebugEvent, ExchangeDebugLog};

// ===== Basic event collection =====

#[test]
fn empty_log_has_no_events() {
    let log = ExchangeDebugLog::new();
    assert!(log.events().is_empty());
    assert!(log.to_jsonl().is_empty());
}

#[test]
fn push_event_records_with_timestamp() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });

    let events = log.events();
    assert_eq!(events.len(), 1);
    match &events[0].event {
        ExchangeDebugEvent::SessionStarted { transport } => {
            assert_eq!(transport, "qr");
        }
        other => panic!("Expected SessionStarted, got {:?}", other),
    }
}

#[test]
fn multiple_events_have_increasing_timestamps() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::KeyAgreementCompleted);

    let events = log.events();
    assert_eq!(events.len(), 3);
    // Timestamps should be non-decreasing
    assert!(events[1].elapsed_ms >= events[0].elapsed_ms);
    assert!(events[2].elapsed_ms >= events[1].elapsed_ms);
}

// ===== Event variant coverage =====

#[test]
fn all_event_variants_are_serializable() {
    let mut log = ExchangeDebugLog::new();

    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "ble".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::KeyAgreementCompleted);
    log.push(ExchangeDebugEvent::ProximityCheckStarted {
        method: "ultrasonic".to_string(),
    });
    log.push(ExchangeDebugEvent::ProximityCheckCompleted {
        confidence: "high".to_string(),
    });
    log.push(ExchangeDebugEvent::ExchangeCompleted);
    log.push(ExchangeDebugEvent::ExchangeFailed {
        error: "timeout".to_string(),
    });

    assert_eq!(log.events().len(), 8);

    let jsonl = log.to_jsonl();
    assert!(!jsonl.is_empty());
    // Each event should be on its own line
    let lines: Vec<&str> = jsonl.lines().collect();
    assert_eq!(lines.len(), 8);

    // Each line should be valid JSON
    for line in &lines {
        let parsed: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Invalid JSON line: {}: {}", line, e));
        // Each should have elapsed_ms and event fields
        assert!(
            parsed.get("elapsed_ms").is_some(),
            "Missing elapsed_ms in: {}",
            line
        );
        assert!(parsed.get("event").is_some(), "Missing event in: {}", line);
    }
}

// ===== JSONL export =====

#[test]
fn jsonl_contains_event_type_tags() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::ExchangeCompleted);

    let jsonl = log.to_jsonl();
    assert!(
        jsonl.contains("qr_scanned"),
        "JSONL should contain qr_scanned tag"
    );
    assert!(
        jsonl.contains("exchange_completed"),
        "JSONL should contain exchange_completed tag"
    );
}

// ===== Markdown export =====

#[test]
fn to_markdown_empty_log() {
    let log = ExchangeDebugLog::new();
    let md = log.to_markdown();
    assert!(md.contains("# Exchange Debug Log"));
    assert!(md.contains("0 events"));
}

#[test]
fn to_markdown_with_events() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::SessionStarted {
        transport: "qr".to_string(),
    });
    log.push(ExchangeDebugEvent::QrGenerated);
    log.push(ExchangeDebugEvent::QrScanned);
    log.push(ExchangeDebugEvent::ExchangeCompleted);

    let md = log.to_markdown();
    assert!(md.contains("# Exchange Debug Log"));
    assert!(md.contains("4 events"));
    assert!(md.contains("| Elapsed"));
    assert!(md.contains("SessionStarted"));
    assert!(md.contains("QrGenerated"));
    assert!(md.contains("ExchangeCompleted"));
}

#[test]
fn to_markdown_includes_event_details() {
    let mut log = ExchangeDebugLog::new();
    log.push(ExchangeDebugEvent::ExchangeFailed {
        error: "timeout".to_string(),
    });

    let md = log.to_markdown();
    assert!(md.contains("timeout"));
}
