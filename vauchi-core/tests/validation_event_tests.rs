// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for validation event dispatch (G4)
//!
//! Verifies that FieldValidated, FieldValidationRevoked, and FieldValidationReset
//! events are dispatched at the correct points in the validation lifecycle.

mod common;

use std::sync::{Arc, Mutex};
use vauchi_core::api::*;

use common::helpers::setup_alice_bob_exchange;

// === Event Variant Construction Tests ===

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_field_validated_event_variant_exists() {
    let event = VauchiEvent::FieldValidated {
        contact_id: "abc123".into(),
        field_id: "email".into(),
        validator_id: "def456".into(),
    };

    if let VauchiEvent::FieldValidated {
        contact_id,
        field_id,
        validator_id,
    } = event
    {
        assert_eq!(contact_id, "abc123");
        assert_eq!(field_id, "email");
        assert_eq!(validator_id, "def456");
    } else {
        panic!("Expected FieldValidated event");
    }
}

// @scenario: field_validation :: Revoke validation
#[test]
fn test_field_validation_revoked_event_variant_exists() {
    let event = VauchiEvent::FieldValidationRevoked {
        contact_id: "abc123".into(),
        field_id: "email".into(),
        validator_id: "def456".into(),
    };

    if let VauchiEvent::FieldValidationRevoked {
        contact_id,
        field_id,
        validator_id,
    } = event
    {
        assert_eq!(contact_id, "abc123");
        assert_eq!(field_id, "email");
        assert_eq!(validator_id, "def456");
    } else {
        panic!("Expected FieldValidationRevoked event");
    }
}

// @scenario: field_validation :: Validation resets when field value changes
#[test]
fn test_field_validation_reset_event_variant_exists() {
    let event = VauchiEvent::FieldValidationReset {
        contact_id: "abc123".into(),
        field_id: "email".into(),
    };

    if let VauchiEvent::FieldValidationReset {
        contact_id,
        field_id,
    } = event
    {
        assert_eq!(contact_id, "abc123");
        assert_eq!(field_id, "email");
    } else {
        panic!("Expected FieldValidationReset event");
    }
}

// @scenario: field_validation :: Validate a contact's social profile
#[test]
fn test_field_validated_event_clone() {
    let event = VauchiEvent::FieldValidated {
        contact_id: "c1".into(),
        field_id: "phone".into(),
        validator_id: "v1".into(),
    };
    let cloned = event.clone();
    assert!(
        matches!(cloned, VauchiEvent::FieldValidated { .. }),
        "Cloned event should be FieldValidated"
    );
}

// === Dispatch on Incoming Validation ===

// @scenario: field_validation :: Validate a contact's social profile
// @scenario: field_validation :: Validation count syncs from contacts
// @scenario: field_validation :: Validations are cryptographically signed
#[test]
fn test_field_validated_event_dispatched_on_incoming_validation() {
    let (alice_wb, bob_wb, _secret, bob_contact_id, alice_contact_id) = setup_alice_bob_exchange();

    // Set up event capture on Alice (she receives the validation)
    let events: Arc<Mutex<Vec<VauchiEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    alice_wb.add_event_handler(Arc::new(CallbackHandler::new(move |event| {
        events_clone.lock().unwrap().push(event);
    })));

    // Bob validates one of Alice's fields — create a signed validation
    let bob_identity = bob_wb.identity().expect("Bob should have identity");
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        bob_identity,
        "email",
        "alice@example.com",
        &alice_contact_id,
    );

    // Serialize the validation and process it on Alice's side
    let validation_bytes = serde_json::to_vec(&validation).expect("validation should serialize");

    alice_wb
        .process_incoming_validation(&bob_contact_id, &validation_bytes)
        .expect("process_incoming_validation should succeed");

    // Assert FieldValidated event was dispatched
    let captured = events.lock().unwrap();
    let field_validated_events: Vec<&VauchiEvent> = captured
        .iter()
        .filter(|e| matches!(e, VauchiEvent::FieldValidated { .. }))
        .collect();

    assert_eq!(
        field_validated_events.len(),
        1,
        "Exactly one FieldValidated event should be dispatched"
    );

    if let VauchiEvent::FieldValidated {
        contact_id,
        field_id,
        validator_id,
    } = field_validated_events[0]
    {
        // contact_id is the contact whose field was validated (Alice, from Alice's perspective)
        assert_eq!(
            contact_id, &alice_contact_id,
            "contact_id should be Alice's contact ID (the validated contact)"
        );
        // field_id should contain the field name
        assert!(
            field_id.contains("email"),
            "field_id should contain 'email', got: {}",
            field_id
        );
        // validator_id should be Bob's contact ID
        assert_eq!(
            validator_id, &bob_contact_id,
            "validator_id should be Bob's contact ID"
        );
    } else {
        panic!("Expected FieldValidated event");
    }
}

// === Dispatch on Incoming Revocation ===

// @scenario: field_validation :: Revoke validation
// @scenario: field_validation :: Validation count syncs from contacts
#[test]
fn test_field_validation_revoked_event_dispatched_on_incoming_revocation() {
    let (alice_wb, bob_wb, _secret, bob_contact_id, alice_contact_id) = setup_alice_bob_exchange();

    // Bob validates Alice's field first
    let bob_identity = bob_wb.identity().expect("Bob should have identity");
    let validation = vauchi_core::social::ProfileValidation::create_signed(
        bob_identity,
        "email",
        "alice@example.com",
        &alice_contact_id,
    );
    let validation_bytes = serde_json::to_vec(&validation).expect("validation should serialize");

    alice_wb
        .process_incoming_validation(&bob_contact_id, &validation_bytes)
        .expect("process_incoming_validation should succeed");

    // Now set up event capture AFTER the validation is stored
    let events: Arc<Mutex<Vec<VauchiEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    alice_wb.add_event_handler(Arc::new(CallbackHandler::new(move |event| {
        events_clone.lock().unwrap().push(event);
    })));

    // Bob revokes his validation of Alice's field.
    // Note: field_id in revocation uses the short form ("email"), matching
    // how revoke_field_validation() queues it. delete_validation() internally
    // prepends contact_id to build the full field_id for the storage lookup.
    let revocation = serde_json::json!({
        "contact_id": alice_contact_id,
        "field_id": "email",
        "validator_id": bob_contact_id,
    });
    let revocation_bytes = serde_json::to_vec(&revocation).expect("revocation should serialize");

    let deleted = alice_wb
        .process_incoming_revocation(&bob_contact_id, &revocation_bytes)
        .expect("process_incoming_revocation should succeed");

    assert!(deleted, "revocation should have deleted a validation");

    // Assert FieldValidationRevoked event was dispatched
    let captured = events.lock().unwrap();
    let revoked_events: Vec<&VauchiEvent> = captured
        .iter()
        .filter(|e| matches!(e, VauchiEvent::FieldValidationRevoked { .. }))
        .collect();

    assert_eq!(
        revoked_events.len(),
        1,
        "Exactly one FieldValidationRevoked event should be dispatched"
    );

    if let VauchiEvent::FieldValidationRevoked {
        contact_id,
        field_id,
        validator_id,
    } = revoked_events[0]
    {
        assert_eq!(
            contact_id, &alice_contact_id,
            "contact_id should be Alice's contact ID"
        );
        assert_eq!(
            field_id, "email",
            "field_id should match the short field name"
        );
        assert_eq!(
            validator_id, &bob_contact_id,
            "validator_id should be Bob's contact ID"
        );
    } else {
        panic!("Expected FieldValidationRevoked event");
    }
}

// @scenario: field_validation :: Cannot forge validations
#[test]
fn test_field_validation_revoked_event_not_dispatched_when_nothing_deleted() {
    let (alice_wb, _bob_wb, _secret, bob_contact_id, alice_contact_id) = setup_alice_bob_exchange();

    // Set up event capture — no validation stored, so revocation deletes nothing
    let events: Arc<Mutex<Vec<VauchiEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    alice_wb.add_event_handler(Arc::new(CallbackHandler::new(move |event| {
        events_clone.lock().unwrap().push(event);
    })));

    let revocation = serde_json::json!({
        "contact_id": alice_contact_id,
        "field_id": "email",
        "validator_id": bob_contact_id,
    });
    let revocation_bytes = serde_json::to_vec(&revocation).expect("revocation should serialize");

    let deleted = alice_wb
        .process_incoming_revocation(&bob_contact_id, &revocation_bytes)
        .expect("process_incoming_revocation should succeed");

    assert!(
        !deleted,
        "revocation should NOT have deleted anything (nothing was stored)"
    );

    // Assert NO FieldValidationRevoked event was dispatched
    let captured = events.lock().unwrap();
    let revoked_events: Vec<&VauchiEvent> = captured
        .iter()
        .filter(|e| matches!(e, VauchiEvent::FieldValidationRevoked { .. }))
        .collect();

    assert_eq!(
        revoked_events.len(),
        0,
        "No FieldValidationRevoked event should be dispatched when nothing was deleted"
    );
}
