//! Tests for network::mock
//! Extracted from mock.rs

use vauchi_core::network::*;

fn create_test_message() -> MessageEnvelope {
    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "test-msg-1".to_string(),
        timestamp: 12345,
        payload: MessagePayload::Presence(PresenceUpdate {
            status: PresenceStatus::Online,
            message: None,
        }),
    }
}

#[test]
fn test_mock_transport_connect_disconnect() {
    let mut transport = MockTransport::new();

    assert_eq!(transport.state(), ConnectionState::Disconnected);

    transport.connect(&TransportConfig::default()).unwrap();
    assert_eq!(transport.state(), ConnectionState::Connected);

    transport.disconnect().unwrap();
    assert_eq!(transport.state(), ConnectionState::Disconnected);
}

#[test]
fn test_mock_transport_send_receive() {
    let mut transport = MockTransport::new();
    transport.connect(&TransportConfig::default()).unwrap();

    // Queue a message to receive
    let incoming = create_test_message();
    transport.queue_receive(incoming.clone());

    // Receive it
    let received = transport.receive().unwrap().unwrap();
    assert_eq!(received.message_id, incoming.message_id);

    // No more messages
    assert!(transport.receive().unwrap().is_none());
}

#[test]
fn test_mock_transport_send_tracks_messages() {
    let mut transport = MockTransport::new();
    transport.connect(&TransportConfig::default()).unwrap();

    let message = create_test_message();
    transport.send(&message).unwrap();

    assert_eq!(transport.sent_messages().len(), 1);
    assert_eq!(transport.sent_messages()[0].message_id, message.message_id);
}

#[test]
fn test_mock_transport_error_injection() {
    let mut transport = MockTransport::new();
    transport.inject_error(NetworkError::ConnectionFailed("test error".into()));

    let result = transport.connect(&TransportConfig::default());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("test error"));
}

#[test]
fn test_mock_transport_auto_ack() {
    let mut transport = MockTransport::new();
    transport.set_auto_ack(true);
    transport.connect(&TransportConfig::default()).unwrap();

    let message = create_test_message();
    transport.send(&message).unwrap();

    // Should have an ack in the receive queue
    assert!(transport.has_pending());
    let ack = transport.receive().unwrap().unwrap();

    if let MessagePayload::Acknowledgment(ack_payload) = ack.payload {
        assert_eq!(ack_payload.message_id, message.message_id);
        assert_eq!(ack_payload.status, AckStatus::Delivered);
    } else {
        panic!("Expected acknowledgment message");
    }
}

#[test]
fn test_mock_transport_not_connected_error() {
    let mut transport = MockTransport::new();

    // Try to send without connecting
    let message = create_test_message();
    let result = transport.send(&message);

    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), NetworkError::NotConnected));
}

#[test]
fn test_mock_transport_clear_sent() {
    let mut transport = MockTransport::new();
    transport.connect(&TransportConfig::default()).unwrap();

    transport.send(&create_test_message()).unwrap();
    assert_eq!(transport.sent_messages().len(), 1);

    transport.clear_sent();
    assert!(transport.sent_messages().is_empty());
}

#[test]
fn test_mock_transport_set_state() {
    let mut transport = MockTransport::new();

    transport.set_state(ConnectionState::Reconnecting { attempt: 3 });
    assert_eq!(
        transport.state(),
        ConnectionState::Reconnecting { attempt: 3 }
    );
}

#[test]
fn test_mock_transport_has_pending() {
    let mut transport = MockTransport::new();
    transport.connect(&TransportConfig::default()).unwrap();

    assert!(!transport.has_pending());

    transport.queue_receive(create_test_message());
    assert!(transport.has_pending());

    transport.receive().unwrap();
    assert!(!transport.has_pending());
}
