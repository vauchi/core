// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::TransportType;
use vauchi_core::exchange::transport::trace::{TraceEventKind, TraceLog};

// @internal
#[test]
fn records_events_and_returns_them() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::HandshakeStarted);
    log.record(TraceEventKind::PeerDiscovered {
        peer_id: "abc".into(),
    });
    log.record(TraceEventKind::ExchangeComplete);

    let events = log.events();
    assert_eq!(events.len(), 3);

    assert!(matches!(events[0].event, TraceEventKind::HandshakeStarted));
    assert!(matches!(
        events[1].event,
        TraceEventKind::PeerDiscovered { .. }
    ));
    assert!(matches!(events[2].event, TraceEventKind::ExchangeComplete));
}

// @internal
#[test]
fn timestamps_are_monotonically_increasing() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::HandshakeStarted);
    log.record(TraceEventKind::SharedKeyDerived);
    log.record(TraceEventKind::ExchangeComplete);

    let events = log.events();
    for window in events.windows(2) {
        assert!(
            window[1].timestamp_us >= window[0].timestamp_us,
            "timestamps must be monotonically increasing: {} < {}",
            window[1].timestamp_us,
            window[0].timestamp_us
        );
    }
}

// @internal
#[test]
fn export_json_contains_expected_strings() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::WifiAwarePublishing);
    log.record(TraceEventKind::TransportSelected {
        selected: TransportType::WifiAware,
    });

    let json = log.export_json();
    assert!(
        json.contains("wifi_aware"),
        "JSON should contain 'wifi_aware', got: {json}"
    );
    assert!(
        json.contains("transport_selected"),
        "JSON should contain 'transport_selected', got: {json}"
    );
    assert!(
        json.contains("wifi_aware_publishing"),
        "JSON should contain 'wifi_aware_publishing', got: {json}"
    );
}

// @internal
#[test]
fn summary_extracts_transport_used_from_transport_selected() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::TransportSelected {
        selected: TransportType::Ble,
    });
    log.record(TraceEventKind::ExchangeComplete);

    let summary = log.summary();
    assert_eq!(summary.transport_used, Some(TransportType::Ble));
}

// @internal
#[test]
fn summary_counts_fallbacks() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::FallbackTriggered {
        from: TransportType::WifiAware,
        to: TransportType::Ble,
        reason: "timeout".into(),
    });
    log.record(TraceEventKind::FallbackTriggered {
        from: TransportType::Ble,
        to: TransportType::AnimatedQr,
        reason: "connection lost".into(),
    });
    log.record(TraceEventKind::ExchangeComplete);

    let summary = log.summary();
    assert_eq!(summary.fallbacks.len(), 2);
    assert_eq!(
        summary.fallbacks[0],
        (TransportType::WifiAware, TransportType::Ble)
    );
    assert_eq!(
        summary.fallbacks[1],
        (TransportType::Ble, TransportType::AnimatedQr)
    );
}

// @internal
#[test]
fn summary_totals_bytes_transferred() {
    let mut log = TraceLog::new();
    log.record(TraceEventKind::KeyOfferSent { size: 100 });
    log.record(TraceEventKind::KeyOfferReceived { size: 120 });
    log.record(TraceEventKind::CardEncrypted { size: 500 });
    log.record(TraceEventKind::CardDecrypted { size: 480 });

    let summary = log.summary();
    assert_eq!(summary.bytes_transferred, 100 + 120 + 500 + 480);
}

// @internal
#[test]
fn empty_log_produces_empty_summary() {
    let log = TraceLog::new();
    let summary = log.summary();

    assert_eq!(summary.transport_used, None);
    assert!(summary.fallbacks.is_empty());
    assert_eq!(summary.total_duration_us, 0);
    assert_eq!(summary.bytes_transferred, 0);
}
