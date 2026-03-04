// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for exchange platform constraints.
//! Covers battery checks, storage checks, clock drift, and blocked contact rejection
//! during the exchange flow.
//!
//! Feature tags: @exchange @constraints @battery @storage @clock @blocked

use vauchi_core::exchange::*;
use vauchi_core::*;

// Mock platform callbacks that can simulate various constraint states.
struct MockPlatformCallbacks {
    battery_ok: bool,
    storage_ok: bool,
}

impl ExchangePlatformCallbacks for MockPlatformCallbacks {
    fn check_battery_level(&self) -> Result<(), ExchangeError> {
        if self.battery_ok {
            Ok(())
        } else {
            Err(ExchangeError::LowBattery)
        }
    }

    fn check_storage_available(&self) -> Result<(), ExchangeError> {
        if self.storage_ok {
            Ok(())
        } else {
            Err(ExchangeError::InsufficientStorage)
        }
    }
}

// ============================================================================
// Battery Level Checks
// ============================================================================

/// Feature: contact_exchange.feature @constraints @battery
/// Scenario: Exchange blocked when battery is critically low
#[test]
fn test_exchange_blocked_when_battery_insufficient() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: false,
        storage_ok: true,
    };

    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    // Start QR exchange
    session.apply(ExchangeEvent::StartQR).unwrap();

    // Attempt to perform pre-exchange checks via platform callbacks
    let result =
        session.apply_with_callbacks(ExchangeEvent::PerformKeyAgreement, &platform_callbacks);

    // Should fail due to low battery
    assert!(matches!(result, Err(ExchangeError::LowBattery)));
}

/// Feature: contact_exchange.feature @constraints @battery
/// Scenario: Exchange proceeds when battery is sufficient
#[test]
fn test_exchange_allowed_when_battery_sufficient() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: true,
        storage_ok: true,
    };

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    // Start and process QR
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Perform key agreement with battery check
    let result =
        bob_session.apply_with_callbacks(ExchangeEvent::PerformKeyAgreement, &platform_callbacks);

    // Should succeed with good battery
    assert!(result.is_ok());
}

// ============================================================================
// Storage Availability Checks
// ============================================================================

/// Feature: contact_exchange.feature @constraints @storage
/// Scenario: Exchange blocked when storage is insufficient
#[test]
fn test_exchange_blocked_when_storage_insufficient() {
    let identity = Identity::create("Alice");
    let card = ContactCard::new("Alice");
    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: true,
        storage_ok: false,
    };

    let mut session = ExchangeSession::new_qr(identity, card, proximity);

    // Start QR exchange
    session.apply(ExchangeEvent::StartQR).unwrap();

    // Attempt key agreement with storage check
    let result =
        session.apply_with_callbacks(ExchangeEvent::PerformKeyAgreement, &platform_callbacks);

    // Should fail due to insufficient storage
    assert!(matches!(result, Err(ExchangeError::InsufficientStorage)));
}

/// Feature: contact_exchange.feature @constraints @storage
/// Scenario: Exchange proceeds when storage is sufficient
#[test]
fn test_exchange_allowed_when_storage_sufficient() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: true,
        storage_ok: true,
    };

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Perform key agreement with storage check
    let result =
        bob_session.apply_with_callbacks(ExchangeEvent::PerformKeyAgreement, &platform_callbacks);

    // Should succeed with sufficient storage
    assert!(result.is_ok());
}

// ============================================================================
// Clock Drift Checks
// ============================================================================

/// Feature: contact_exchange.feature @constraints @clock
/// Scenario: QR is rejected if system clock is skewed (clock drift > 30 seconds)
#[test]
fn test_process_qr_rejects_large_clock_drift() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    // Create a QR with a timestamp far in the future (>120 seconds ahead)
    // This simulates Alice's device having a clock 180 seconds ahead of Bob's
    let future_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + 180; // 180 seconds in the future (> 120s max drift tolerance)

    let alice_qr =
        ExchangeQR::generate_with_timestamp(&alice_identity, &alice_ephemeral, future_timestamp);

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Processing the QR with large clock drift should fail
    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));

    // Should be rejected due to clock drift
    assert!(matches!(result, Err(ExchangeError::ClockDrift(_))));
}

/// Feature: contact_exchange.feature @constraints @clock
/// Scenario: QR is accepted if clock drift is within tolerance (±120 seconds)
#[test]
fn test_process_qr_accepts_small_clock_drift() {
    use std::time::{SystemTime, UNIX_EPOCH};

    let alice_identity = Identity::create("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    // Create QR with a small clock drift (15 seconds behind current time)
    let current_timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Use a timestamp 15 seconds in the past (within ±30s tolerance)
    let slightly_old_timestamp = current_timestamp - 15;
    let alice_qr = ExchangeQR::generate_with_timestamp(
        &alice_identity,
        &alice_ephemeral,
        slightly_old_timestamp,
    );

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");
    let proximity = MockProximityVerifier::success();

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);

    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Processing the QR with acceptable clock drift should succeed
    let result = bob_session.apply(ExchangeEvent::ProcessQR(alice_qr));

    // Should succeed
    assert!(result.is_ok());
    assert!(matches!(
        bob_session.state(),
        ExchangeState::PeerScanned { .. }
    ));
}

// ============================================================================
// Blocked Contact Checks
// ============================================================================

/// Feature: contact_exchange.feature @constraints @blocked
/// Scenario: Exchange auto-completes as rejected contact if peer is in blocked list
#[test]
fn test_exchange_rejects_blocked_contact() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Mark Alice as blocked (simulated via test helper)
    // When Bob completes the exchange, the contact should be marked as rejected
    let result = bob_session.apply_with_blocked_list(
        ExchangeEvent::CompleteExchange(alice_card),
        &[*alice_identity.signing_public_key()],
    );

    // Exchange should complete but contact marked as blocked
    assert!(result.is_ok());

    if let Ok(contact) = result {
        // Contact should be marked as rejected/blocked
        assert!(contact.is_blocked());
    }
}

/// Feature: contact_exchange.feature @constraints @blocked
/// Scenario: Exchange proceeds normally for non-blocked contacts
#[test]
fn test_exchange_succeeds_for_unblocked_contact() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Complete exchange with empty blocked list
    let result = bob_session.apply_with_blocked_list(
        ExchangeEvent::CompleteExchange(alice_card),
        &[], // No blocked contacts
    );

    // Should succeed and contact should NOT be blocked
    assert!(result.is_ok());

    if let Ok(contact) = result {
        assert!(!contact.is_blocked());
    }
}

// ============================================================================
// Production Blocked Contact Enforcement (apply_with_callbacks_and_blocked)
// ============================================================================

/// Feature: contact_exchange.feature @constraints @blocked
/// Scenario: Production path rejects blocked contact via apply_with_callbacks_and_blocked
#[test]
fn test_production_blocked_contact_rejection() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: true,
        storage_ok: true,
    };

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply(ExchangeEvent::PerformKeyAgreement)
        .unwrap();

    // Use production method with blocked list
    let result = bob_session.apply_with_callbacks_and_blocked(
        ExchangeEvent::CompleteExchange(alice_card),
        &platform_callbacks,
        &[*alice_identity.signing_public_key()],
    );

    // Production path should return ContactBlocked error
    assert!(matches!(result, Err(ExchangeError::ContactBlocked)));
}

/// Feature: contact_exchange.feature @constraints @blocked
/// Scenario: Production path allows non-blocked contacts through
#[test]
fn test_production_unblocked_contact_succeeds() {
    let alice_identity = Identity::create("Alice");
    let alice_card = ContactCard::new("Alice");
    let alice_ephemeral = X3DHKeyPair::generate();

    let bob_identity = Identity::create("Bob");
    let bob_card = ContactCard::new("Bob");

    let proximity = MockProximityVerifier::success();
    let platform_callbacks = MockPlatformCallbacks {
        battery_ok: true,
        storage_ok: true,
    };

    let alice_qr = ExchangeQR::generate(&alice_identity, &alice_ephemeral);

    let mut bob_session = ExchangeSession::new_qr(bob_identity, bob_card, proximity);
    bob_session.apply(ExchangeEvent::StartQR).unwrap();
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    bob_session
        .apply_with_callbacks(ExchangeEvent::PerformKeyAgreement, &platform_callbacks)
        .unwrap();

    // Production path with empty blocked list should succeed
    let result = bob_session.apply_with_callbacks_and_blocked(
        ExchangeEvent::CompleteExchange(alice_card),
        &platform_callbacks,
        &[], // No blocked contacts
    );

    assert!(result.is_ok());
}
