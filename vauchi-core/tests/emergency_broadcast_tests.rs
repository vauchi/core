// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency Broadcast Tests
//!
//! TDD tests for the emergency broadcast system:
//! - Config storage (CRUD for emergency broadcast configuration)
//! - Alert types (EmergencyAlert serialization, GeoLocation optional)
//! - Broadcast API (configure, send, event dispatch)
//! - Migration V22 (emergency_config table)
//!
//! Reference: features/emergency_broadcast.feature

mod common;

use std::sync::{Arc, Mutex};

use vauchi_core::types::EmergencyBroadcastConfig;
use vauchi_core::{EmergencyAlert, GeoLocation, Storage, SymmetricKey, VauchiEvent};

use common::helpers::{create_vauchi_with_identity, setup_alice_bob_exchange, setup_ratchets};

// =============================================================================
// Config Storage Tests
// =============================================================================

// @scenario: emergency_broadcast :: Emergency broadcast is opt-in
#[test]
fn test_load_emergency_config_returns_none_initially() {
    let wb = create_vauchi_with_identity("Alice");

    let config = wb.load_emergency_config().expect("load should succeed");
    assert!(
        config.is_none(),
        "emergency config should be None before any configuration"
    );
}

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
// @scenario: emergency_broadcast :: Configure alert message
// @scenario: emergency_broadcast :: Configure location sharing for alerts
#[test]
fn test_save_load_emergency_config_roundtrip() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.configure_emergency_broadcast(
        vec!["contact-1".to_string(), "contact-2".to_string()],
        "I may be in danger. Please check on me.".to_string(),
        true,
    )
    .expect("configure should succeed");

    let config = wb
        .load_emergency_config()
        .expect("load should succeed")
        .expect("should have emergency config");

    assert_eq!(
        config.trusted_contact_ids,
        vec!["contact-1".to_string(), "contact-2".to_string()]
    );
    assert_eq!(config.message, "I may be in danger. Please check on me.");
    assert!(config.include_location);
}

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn test_delete_emergency_config() {
    let mut wb = create_vauchi_with_identity("Alice");

    wb.configure_emergency_broadcast(vec!["contact-1".to_string()], "Help!".to_string(), false)
        .expect("configure should succeed");

    wb.delete_emergency_config().expect("delete should succeed");

    let config = wb.load_emergency_config().expect("load should succeed");
    assert!(
        config.is_none(),
        "emergency config should be None after delete"
    );
}

// =============================================================================
// Alert Types Tests
// =============================================================================

// @scenario: emergency_broadcast :: Alert message content
// @scenario: emergency_broadcast :: Include location when enabled
#[test]
fn test_emergency_alert_serialization() {
    let alert = EmergencyAlert {
        sender_id: "sender-abc".to_string(),
        message: "I may be in danger".to_string(),
        timestamp: 1700000000,
        location: Some(GeoLocation {
            latitude: 47.3769,
            longitude: 8.5417,
            accuracy_meters: Some(10.0),
        }),
    };

    let json = serde_json::to_string(&alert).expect("serialize should succeed");
    let deserialized: EmergencyAlert =
        serde_json::from_str(&json).expect("deserialize should succeed");

    assert_eq!(deserialized.sender_id, "sender-abc");
    assert_eq!(deserialized.message, "I may be in danger");
    assert_eq!(deserialized.timestamp, 1700000000);

    let loc = deserialized.location.expect("location should be present");
    assert!((loc.latitude - 47.3769).abs() < f64::EPSILON);
    assert!((loc.longitude - 8.5417).abs() < f64::EPSILON);
    assert!((loc.accuracy_meters.unwrap() - 10.0).abs() < f32::EPSILON);
}

// @scenario: emergency_broadcast :: Location disabled by default
#[test]
fn test_geo_location_optional() {
    let alert = EmergencyAlert {
        sender_id: "sender-xyz".to_string(),
        message: "Emergency".to_string(),
        timestamp: 1700000001,
        location: None,
    };

    let json = serde_json::to_string(&alert).expect("serialize should succeed");
    let deserialized: EmergencyAlert =
        serde_json::from_str(&json).expect("deserialize should succeed");

    assert!(deserialized.location.is_none());
}

// =============================================================================
// Broadcast API Tests
// =============================================================================

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn test_configure_emergency_broadcast() {
    let mut wb = create_vauchi_with_identity("Alice");

    let result = wb.configure_emergency_broadcast(
        vec!["c1".to_string(), "c2".to_string()],
        "Help me!".to_string(),
        false,
    );
    assert!(result.is_ok(), "configure should succeed");

    let config = wb
        .load_emergency_config()
        .expect("load should succeed")
        .expect("should have config");

    assert_eq!(config.trusted_contact_ids.len(), 2);
    assert_eq!(config.message, "Help me!");
    assert!(!config.include_location);
}

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn test_configure_emergency_broadcast_max_10_contacts() {
    let mut wb = create_vauchi_with_identity("Alice");

    // Attempting to configure with more than 10 contacts should fail
    let ids: Vec<String> = (0..11).map(|i| format!("contact-{}", i)).collect();
    let result = wb.configure_emergency_broadcast(ids, "Help!".to_string(), false);
    assert!(
        result.is_err(),
        "configuring with more than 10 trusted contacts should be rejected"
    );
}

// @scenario: emergency_broadcast :: No trusted contacts configured
#[test]
fn test_send_emergency_broadcast_without_config_fails() {
    let mut wb = create_vauchi_with_identity("Alice");

    let result = wb.send_emergency_broadcast();
    assert!(
        result.is_err(),
        "sending broadcast without config should fail"
    );
}

// @scenario: emergency_broadcast :: Send emergency broadcast from app
// @scenario: emergency_broadcast :: Alert delivery to multiple contacts
#[test]
fn test_send_emergency_broadcast_queues_alerts() {
    let (mut alice_wb, _bob_wb, secret, bob_id, _alice_id) = setup_alice_bob_exchange();

    // Set up ratchet state so Alice can encrypt messages to Bob
    let (alice_ratchet, _bob_ratchet) = setup_ratchets(&secret);
    alice_wb
        .save_ratchet_state(&bob_id, &alice_ratchet)
        .expect("save ratchet should succeed");

    // Configure emergency broadcast with Bob as trusted contact
    alice_wb
        .configure_emergency_broadcast(
            vec![bob_id.clone()],
            "I may be in danger. Please check on me.".to_string(),
            false,
        )
        .expect("configure should succeed");

    let result = alice_wb
        .send_emergency_broadcast()
        .expect("broadcast should succeed");

    assert_eq!(result.sent, 1, "should send to 1 contact (Bob)");
    assert_eq!(result.total, 1, "total should be 1");
}

// @scenario: emergency_broadcast :: Alert delivery to multiple contacts
#[test]
fn test_send_emergency_broadcast_returns_result() {
    let (mut alice_wb, _bob_wb, secret, bob_id, _alice_id) = setup_alice_bob_exchange();

    // Set up ratchet state so Alice can encrypt messages to Bob
    let (alice_ratchet, _bob_ratchet) = setup_ratchets(&secret);
    alice_wb
        .save_ratchet_state(&bob_id, &alice_ratchet)
        .expect("save ratchet should succeed");

    // Configure with Bob as trusted contact + a nonexistent one
    alice_wb
        .configure_emergency_broadcast(
            vec![bob_id, "nonexistent-contact".to_string()],
            "Help!".to_string(),
            false,
        )
        .expect("configure should succeed");

    let result = alice_wb
        .send_emergency_broadcast()
        .expect("broadcast should succeed");

    // Bob is a real contact, nonexistent-contact is not — sent count reflects only Bob
    assert_eq!(result.total, 2, "total should match config");
    assert_eq!(result.sent, 1, "sent should only count reachable contacts");
}

// @scenario: emergency_broadcast :: Send emergency broadcast from app
#[test]
fn test_broadcast_dispatches_event() {
    let (mut alice_wb, _bob_wb, secret, bob_id, _alice_id) = setup_alice_bob_exchange();

    // Set up ratchet state so Alice can encrypt messages to Bob
    let (alice_ratchet, _bob_ratchet) = setup_ratchets(&secret);
    alice_wb
        .save_ratchet_state(&bob_id, &alice_ratchet)
        .expect("save ratchet should succeed");

    let events: Arc<Mutex<Vec<VauchiEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    alice_wb.add_event_handler(Arc::new(move |event: VauchiEvent| {
        events_clone.lock().unwrap().push(event);
    }));

    alice_wb
        .configure_emergency_broadcast(vec![bob_id], "Emergency!".to_string(), false)
        .expect("configure should succeed");

    alice_wb
        .send_emergency_broadcast()
        .expect("broadcast should succeed");

    let captured = events.lock().unwrap();
    let broadcast_events: Vec<&VauchiEvent> = captured
        .iter()
        .filter(|e| matches!(e, VauchiEvent::EmergencyBroadcastSent { .. }))
        .collect();

    assert!(
        !broadcast_events.is_empty(),
        "should have dispatched EmergencyBroadcastSent event"
    );

    if let VauchiEvent::EmergencyBroadcastSent { sent_count, total } = broadcast_events[0] {
        assert_eq!(*sent_count, 1);
        assert_eq!(*total, 1);
    } else {
        panic!("unexpected event variant");
    }
}

// =============================================================================
// Storage-Level Config Tests
// =============================================================================

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn test_storage_save_load_emergency_config_roundtrip() {
    let storage = Storage::in_memory(SymmetricKey::generate()).expect("storage should open");

    let config = EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["id-1".to_string(), "id-2".to_string()],
        message: "SOS".to_string(),
        include_location: true,
    };

    storage
        .save_emergency_config(&config)
        .expect("save should succeed");

    let loaded = storage
        .load_emergency_config()
        .expect("load should succeed")
        .expect("should have config");

    assert_eq!(loaded.trusted_contact_ids, config.trusted_contact_ids);
    assert_eq!(loaded.message, config.message);
    assert_eq!(loaded.include_location, config.include_location);
}

// @scenario: emergency_broadcast :: Emergency broadcast is opt-in
#[test]
fn test_storage_load_emergency_config_returns_none_initially() {
    let storage = Storage::in_memory(SymmetricKey::generate()).expect("storage should open");

    let config = storage
        .load_emergency_config()
        .expect("load should succeed");
    assert!(config.is_none());
}

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn test_storage_delete_emergency_config() {
    let storage = Storage::in_memory(SymmetricKey::generate()).expect("storage should open");

    let config = EmergencyBroadcastConfig {
        trusted_contact_ids: vec!["id-1".to_string()],
        message: "Help!".to_string(),
        include_location: false,
    };

    storage
        .save_emergency_config(&config)
        .expect("save should succeed");

    storage
        .delete_emergency_config()
        .expect("delete should succeed");

    let loaded = storage
        .load_emergency_config()
        .expect("load should succeed");
    assert!(loaded.is_none());
}
