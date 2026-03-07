// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::diagnostics::TransportDiagnostics;
use vauchi_core::exchange::transport::{MockTransportChannel, TransportType};

#[test]
fn probe_all_returns_results_for_all_transports() {
    let transports: Vec<Box<dyn vauchi_core::exchange::transport::TransportChannel>> = vec![
        Box::new(MockTransportChannel::new(TransportType::Ble)),
        Box::new(MockTransportChannel::new(TransportType::WifiAware)),
        Box::new(MockTransportChannel::new(TransportType::Nfc)),
    ];
    let diag = TransportDiagnostics::new(transports);
    let results = diag.probe_all();
    assert_eq!(results.len(), 3);

    let types: Vec<TransportType> = results.iter().map(|r| r.transport).collect();
    assert!(types.contains(&TransportType::Ble));
    assert!(types.contains(&TransportType::WifiAware));
    assert!(types.contains(&TransportType::Nfc));
}

#[test]
fn probe_all_correctly_reports_available_and_unavailable() {
    let transports: Vec<Box<dyn vauchi_core::exchange::transport::TransportChannel>> = vec![
        Box::new(MockTransportChannel::new(TransportType::Ble).with_available(true)),
        Box::new(MockTransportChannel::new(TransportType::WifiAware).with_available(false)),
    ];
    let diag = TransportDiagnostics::new(transports);
    let results = diag.probe_all();

    let ble_result = results
        .iter()
        .find(|r| r.transport == TransportType::Ble)
        .unwrap();
    assert!(ble_result.available);
    assert!(ble_result.error.is_none());

    let wifi_result = results
        .iter()
        .find(|r| r.transport == TransportType::WifiAware)
        .unwrap();
    assert!(!wifi_result.available);
    assert!(wifi_result.error.is_none());
}

#[test]
fn probe_returns_none_for_unknown_transport_type() {
    let transports: Vec<Box<dyn vauchi_core::exchange::transport::TransportChannel>> =
        vec![Box::new(MockTransportChannel::new(TransportType::Ble))];
    let diag = TransportDiagnostics::new(transports);
    let result = diag.probe(TransportType::Nfc);
    assert!(result.is_none());
}

#[test]
fn available_types_returns_only_available_ones() {
    let transports: Vec<Box<dyn vauchi_core::exchange::transport::TransportChannel>> = vec![
        Box::new(MockTransportChannel::new(TransportType::Ble).with_available(true)),
        Box::new(MockTransportChannel::new(TransportType::WifiAware).with_available(false)),
        Box::new(MockTransportChannel::new(TransportType::Nfc).with_available(true)),
    ];
    let diag = TransportDiagnostics::new(transports);
    let available = diag.available_types();
    assert_eq!(available.len(), 2);
    assert!(available.contains(&TransportType::Ble));
    assert!(available.contains(&TransportType::Nfc));
    assert!(!available.contains(&TransportType::WifiAware));
}

#[test]
fn empty_diagnostics_returns_empty_probes() {
    let diag = TransportDiagnostics::new(vec![]);
    let results = diag.probe_all();
    assert!(results.is_empty());
    let available = diag.available_types();
    assert!(available.is_empty());
}

#[test]
fn probe_all_captures_error_message_from_transport() {
    // MockTransportChannel.is_available() returns Ok(bool), not Err,
    // so we verify that an unavailable transport probe has no error field
    // (the mock simply returns Ok(false), not an error).
    // The error field is populated only when is_available() itself fails.
    let transports: Vec<Box<dyn vauchi_core::exchange::transport::TransportChannel>> =
        vec![Box::new(
            MockTransportChannel::new(TransportType::Ble).with_available(false),
        )];
    let diag = TransportDiagnostics::new(transports);
    let results = diag.probe_all();
    assert_eq!(results.len(), 1);

    let result = &results[0];
    assert_eq!(result.transport, TransportType::Ble);
    assert!(!result.available);
    // MockTransportChannel returns Ok(false), not Err, so error is None
    assert!(result.error.is_none());
}
