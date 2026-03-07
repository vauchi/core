// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use vauchi_core::exchange::transport::{
    mock::MockTransportChannel,
    orchestrator::{FallbackPolicy, TransportChain},
    TransportChannel, TransportType,
};

#[test]
fn chain_selects_first_available_transport() {
    let ble = MockTransportChannel::new(TransportType::Ble).with_available(true);
    let tcp = MockTransportChannel::new(TransportType::Tcp).with_available(true);

    let chain = TransportChain::new(
        vec![Box::new(ble), Box::new(tcp)],
        FallbackPolicy::PreserveSession,
    );

    let selected = chain.select_transport().expect("should select a transport");
    assert_eq!(selected.transport_type(), TransportType::Ble);
}

#[test]
fn chain_falls_back_when_first_unavailable() {
    let ble = MockTransportChannel::new(TransportType::Ble).with_available(false);
    let tcp = MockTransportChannel::new(TransportType::Tcp).with_available(true);

    let chain = TransportChain::new(
        vec![Box::new(ble), Box::new(tcp)],
        FallbackPolicy::RestartHandshake,
    );

    let selected = chain.select_transport().expect("should fall back to tcp");
    assert_eq!(selected.transport_type(), TransportType::Tcp);
}

#[test]
fn chain_errors_when_none_available() {
    let ble = MockTransportChannel::new(TransportType::Ble).with_available(false);
    let tcp = MockTransportChannel::new(TransportType::Tcp).with_available(false);

    let chain = TransportChain::new(
        vec![Box::new(ble), Box::new(tcp)],
        FallbackPolicy::PreserveSession,
    );

    let result = chain.select_transport();
    assert!(result.is_err());
    let err = result.err().expect("should be an error");
    let err_msg = format!("{err}");
    assert!(
        err_msg.contains("no common transport"),
        "expected NoCommonTransport error, got: {err_msg}"
    );
}

#[test]
fn mock_send_receive_roundtrip() {
    let mock = MockTransportChannel::new(TransportType::Ble).with_available(true);

    let payload = b"hello vauchi";
    mock.queue_receive(payload.to_vec());

    mock.send(payload).expect("send should succeed");

    let received = mock
        .receive(std::time::Duration::from_secs(1))
        .expect("receive should succeed");
    assert_eq!(received, payload.to_vec());

    let sent = mock.sent_data();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0], payload.to_vec());
}

#[test]
fn chain_send_with_fallback_skips_failed_transport() {
    let ble = MockTransportChannel::new(TransportType::Ble)
        .with_available(true)
        .with_send_error("BLE connection lost");
    let tcp = MockTransportChannel::new(TransportType::Tcp).with_available(true);

    let chain = TransportChain::new(
        vec![Box::new(ble), Box::new(tcp)],
        FallbackPolicy::RestartHandshake,
    );

    let payload = b"fallback data";
    let used = chain
        .send_with_fallback(payload)
        .expect("should succeed via fallback");
    assert_eq!(used.transport_type(), TransportType::Tcp);
}

#[test]
fn available_transports_returns_correct_list() {
    let ble = MockTransportChannel::new(TransportType::Ble).with_available(true);
    let tcp = MockTransportChannel::new(TransportType::Tcp).with_available(false);
    let nfc = MockTransportChannel::new(TransportType::Nfc).with_available(true);

    let chain = TransportChain::new(
        vec![Box::new(ble), Box::new(tcp), Box::new(nfc)],
        FallbackPolicy::PreserveSession,
    );

    let available = chain.available_transports();
    assert_eq!(available.len(), 2);
    assert_eq!(available[0], TransportType::Ble);
    assert_eq!(available[1], TransportType::Nfc);
}

#[test]
fn mock_with_send_error_causes_send_to_fail() {
    let mock = MockTransportChannel::new(TransportType::Tcp)
        .with_available(true)
        .with_send_error("network down");

    let result = mock.send(b"data");
    assert!(result.is_err());
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("network down"),
        "expected 'network down' in error, got: {err_msg}"
    );
}

#[test]
fn chain_policy_is_preserved() {
    let chain = TransportChain::new(vec![], FallbackPolicy::PreserveSession);
    assert!(matches!(chain.policy(), FallbackPolicy::PreserveSession));

    let chain2 = TransportChain::new(vec![], FallbackPolicy::RestartHandshake);
    assert!(matches!(chain2.policy(), FallbackPolicy::RestartHandshake));
}
