// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Vauchi::accept_relay_exchange() API
//!
//! Verifies that the high-level exchange API correctly performs
//! X3DH key agreement, creates the contact, and initializes
//! the Double Ratchet — all within core (ADR-021 compliance).

use vauchi_core::Vauchi;
use vauchi_core::VauchiError;
use vauchi_core::exchange::{X3DH, X3DHKeyPair};

/// Helper: create a Vauchi instance with identity.
fn setup_vauchi(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

// @scenario: field_validation.feature - Accept relay exchange creates contact
// @internal
#[test]
fn test_accept_relay_exchange_creates_contact() {
    let wb = setup_vauchi("Alice");

    // Simulate a remote peer (Bob) who initiated exchange
    let bob_identity = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();

    let alice_identity = wb.identity().unwrap();
    let alice_x3dh = alice_identity.x3dh_keypair();

    // Bob initiates X3DH toward Alice
    let (_, bob_ephemeral_pub) = X3DH::initiate(&bob_ephemeral, alice_x3dh.public_key()).unwrap();

    // Alice accepts via the new API
    let contact_id = wb
        .accept_relay_exchange(bob_identity.public_key(), &bob_ephemeral_pub, "Bob")
        .unwrap();

    // Verify contact was created
    let contact = wb.get_contact(&contact_id).unwrap();
    assert!(contact.is_some(), "Contact must exist after accept");
    assert_eq!(
        contact.unwrap().display_name(),
        "Bob",
        "Contact display name must match"
    );
}

// @scenario: field_validation.feature - Accept relay exchange initializes ratchet
// @internal
#[test]
fn test_accept_relay_exchange_initializes_ratchet() {
    let wb = setup_vauchi("Alice");

    let bob_identity = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();

    let alice_identity = wb.identity().unwrap();
    let alice_x3dh = alice_identity.x3dh_keypair();

    let (_, bob_ephemeral_pub) = X3DH::initiate(&bob_ephemeral, alice_x3dh.public_key()).unwrap();

    let contact_id = wb
        .accept_relay_exchange(bob_identity.public_key(), &bob_ephemeral_pub, "Bob")
        .unwrap();

    // Verify ratchet was initialized (can load from storage)
    let ratchet = wb.storage().load_ratchet_state(&contact_id);
    assert!(
        ratchet.is_ok() && ratchet.unwrap().is_some(),
        "Ratchet must be initialized after accept_relay_exchange"
    );
}

// @scenario: field_validation.feature - Accept duplicate exchange fails
// @internal
#[test]
fn test_accept_relay_exchange_rejects_duplicate() {
    let wb = setup_vauchi("Alice");

    let bob_identity = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();

    let alice_identity = wb.identity().unwrap();
    let alice_x3dh = alice_identity.x3dh_keypair();

    let (_, bob_ephemeral_pub) = X3DH::initiate(&bob_ephemeral, alice_x3dh.public_key()).unwrap();

    // First exchange succeeds
    wb.accept_relay_exchange(bob_identity.public_key(), &bob_ephemeral_pub, "Bob")
        .unwrap();

    // Second exchange with same identity key must fail
    let result = wb.accept_relay_exchange(bob_identity.public_key(), &bob_ephemeral_pub, "Bob");
    assert!(result.is_err(), "Duplicate exchange must be rejected");
}

// @scenario: field_validation.feature - Accept exchange without identity fails
// @internal
#[test]
fn test_accept_relay_exchange_requires_identity() {
    let wb = Vauchi::in_memory().unwrap();
    // No identity created

    let bob_identity = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();

    let result =
        wb.accept_relay_exchange(bob_identity.public_key(), bob_ephemeral.public_key(), "Bob");
    assert!(result.is_err(), "Exchange without identity must fail");
}

// @scenario: contact_recovery.feature - Relay contacts cannot be recovery-trusted
//
// Principle 2: "Trust is earned in person."
// A contact created via relay exchange has Standard trust (no proximity
// verification). Granting recovery trust to such a contact would allow
// an unverified remote party to vouch for identity restoration.
// @internal
#[test]
fn test_relay_contact_cannot_be_recovery_trusted() {
    let wb = setup_vauchi("Alice");

    let bob_identity = X3DHKeyPair::generate();
    let bob_ephemeral = X3DHKeyPair::generate();

    let alice_identity = wb.identity().unwrap();
    let alice_x3dh = alice_identity.x3dh_keypair();

    let (_, bob_ephemeral_pub) = X3DH::initiate(&bob_ephemeral, alice_x3dh.public_key()).unwrap();

    let contact_id = wb
        .accept_relay_exchange(bob_identity.public_key(), &bob_ephemeral_pub, "Bob")
        .unwrap();

    // Relay-exchanged contact has Standard trust — no proximity
    let contact = wb.get_contact(&contact_id).unwrap().unwrap();
    assert_eq!(
        contact.trust_level(),
        vauchi_core::contact::TrustLevel::Standard,
        "Relay-exchanged contact must have Standard trust"
    );

    // Attempting to grant recovery trust must fail
    let result = wb.toggle_recovery_trust(&contact_id);
    assert!(
        result.is_err(),
        "Standard-trust contact must not be grantable recovery trust"
    );
    match result.unwrap_err() {
        VauchiError::InvalidState(msg) => {
            assert!(
                msg.contains("trust"),
                "Error must mention trust requirement: {msg}"
            );
        }
        other => panic!("Expected InvalidState, got: {other:?}"),
    }
}
