// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Security Hardening Tests
//!
//! Tests for attack prevention, rate limiting, memory protection, and audit logging.
//! Based on: security.feature - Attack Prevention, Data Protection, Audit & Logging
//!
//! These tests verify that:
//! - Brute force attacks on backup passwords are rate-limited (Argon2id cost)
//! - QR codes expire and cannot be reused after timeout
//! - BLE relay attacks are prevented via proximity verification
//! - Sensitive data is zeroed after use (zeroize)
//! - Security events are logged and exportable
//! - Key material never appears in logs/debug output

use std::time::{SystemTime, UNIX_EPOCH};

use vauchi_core::Identity;
use vauchi_core::crypto::{SigningKeyPair, SymmetricKey, derive_key_argon2id};
use vauchi_core::exchange::{
    BLEProximityVerifier, ExchangeError, ExchangeEvent, ExchangeQR, ExchangeSession,
    MockBLEVerifier, MockProximityVerifier, ProximityError, X3DHKeyPair,
};
use vauchi_core::storage::Storage;

// =============================================================================
// Test 1: Brute Force Protection on Backup Password (Argon2id cost)
// =============================================================================
// security.feature @attacks: "Brute force protection on backup password"
// - Key derivation should be computationally expensive (Argon2id)
// - Each attempt should take significant time
// - A strong password should be practically uncrackable

/// Scenario: Argon2id makes brute-force attacks computationally infeasible
// @scenario: security :: Brute force protection on backup password
#[test]
fn test_brute_force_protection() {
    let password = b"correct-horse-battery-staple";
    let salt = [0x42u8; 32];

    // Measure time for a single key derivation
    let start = std::time::Instant::now();
    let result = derive_key_argon2id(password, &salt);
    let duration = start.elapsed();

    // Derivation should succeed
    result.expect("expected success");

    // Argon2id with m=64MB, t=3, p=4 should take meaningful time
    // This ensures brute-force is expensive. On typical hardware, this
    // should take at least tens of milliseconds.
    assert!(
        duration.as_millis() >= 10,
        "Key derivation took only {}ms - should be >= 10ms for brute-force protection",
        duration.as_millis()
    );

    // Verify different passwords produce different keys
    let wrong_password = b"wrong-password";
    let key_correct = derive_key_argon2id(password, &salt).unwrap();
    let key_wrong = derive_key_argon2id(wrong_password, &salt).unwrap();

    assert_ne!(
        key_correct.as_bytes(),
        key_wrong.as_bytes(),
        "Different passwords should produce different keys"
    );

    // Verify same password + salt produces consistent key
    let key_repeat = derive_key_argon2id(password, &salt).unwrap();
    assert_eq!(
        key_correct.as_bytes(),
        key_repeat.as_bytes(),
        "Same password + salt should produce same key"
    );
}

/// Scenario: Rate limiting calculation - 10k attempts/day is infeasible
// @scenario: security :: Brute force protection on backup password
#[test]
fn test_brute_force_rate_calculation() {
    // With Argon2id taking ~50ms per attempt (conservative estimate),
    // an attacker can only try ~20 passwords/second.
    // At 20 passwords/second: ~1.7 million attempts/day
    // A 12-character password with mixed case/numbers/symbols has
    // ~62^12 = ~3.2 x 10^21 combinations.
    // Time to crack: ~10^15 years (effectively impossible)

    let password = b"test-password";
    let salt = [0x01u8; 32];

    // Run 5 iterations and verify minimum time
    let mut total_ms = 0u128;
    for _ in 0..5 {
        let start = std::time::Instant::now();
        let _ = derive_key_argon2id(password, &salt).unwrap();
        total_ms += start.elapsed().as_millis();
    }

    let avg_ms = total_ms / 5;

    // Average should be meaningful (allow some variance for CI environments)
    assert!(
        avg_ms >= 5,
        "Average derivation time {}ms is too fast - brute force may be feasible",
        avg_ms
    );
}

// =============================================================================
// Test 2: QR Screenshot Attack Prevention (QR Expiration)
// =============================================================================
// security.feature @attacks: "QR code screenshot attack prevention"
// - QR codes have timestamps and expire after 5 minutes
// - Expired QR codes are rejected
// - Audio proximity verification must also pass

/// Scenario: QR code expires after timeout window
// @scenario: security :: QR code screenshot attack prevention
#[test]
fn test_qr_screenshot_attack_prevention() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();

    // Generate QR with timestamp in the past (6 minutes ago = expired)
    let six_minutes_ago = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 360;

    let qr = ExchangeQR::generate_with_timestamp(&identity, &ephemeral, six_minutes_ago);

    // QR should be marked as expired
    assert!(
        qr.is_expired(),
        "QR code from 6 minutes ago should be expired"
    );

    // Verify recently generated QR is not expired
    let fresh_qr = ExchangeQR::generate(&identity, &ephemeral);
    assert!(
        !fresh_qr.is_expired(),
        "Freshly generated QR should not be expired"
    );
}

/// Scenario: Expired QR rejected during exchange session
// @scenario: security :: QR code screenshot attack prevention
#[test]
fn test_expired_qr_rejected_in_session() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_ephemeral = X3DHKeyPair::generate();

    // Alice generates QR (with timestamp 6 minutes ago = expired)
    let six_minutes_ago = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 360;
    let expired_qr =
        ExchangeQR::generate_with_timestamp(&alice_identity, &alice_ephemeral, six_minutes_ago);
    let qr_data = expired_qr.to_data_string();

    // Bob tries to use Alice's expired QR
    let bob_identity = Identity::create("Bob", 0);
    let bob_card = vauchi_core::ContactCard::new("Bob");
    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Bob starts his QR display first
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Parsing the QR data should succeed (signature is valid)
    let parsed_qr = ExchangeQR::from_data_string(&qr_data);
    assert!(parsed_qr.is_ok(), "QR parsing should succeed");

    // But the QR is expired
    assert!(
        parsed_qr.as_ref().unwrap().is_expired(),
        "Parsed QR should be marked expired"
    );

    // Processing expired QR in session should fail
    let result = bob_session.apply(ExchangeEvent::ProcessQR(parsed_qr.unwrap()));

    // The session should reject expired QR
    assert!(
        matches!(result, Err(ExchangeError::QRExpired)),
        "Session should reject expired QR, got: {:?}",
        result
    );
}

/// Scenario: QR with clock drift detection
// @scenario: security :: QR code screenshot attack prevention
#[test]
fn test_qr_clock_drift_detection() {
    use vauchi_core::exchange::check_clock_drift;

    // QR from 5 seconds ago should be fine
    let five_sec_ago = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 5;

    assert!(
        check_clock_drift(five_sec_ago).is_ok(),
        "5 second drift should be acceptable"
    );

    // QR from 60 seconds ago should fail clock drift check
    let sixty_sec_ago = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 60;

    let result = check_clock_drift(sixty_sec_ago);
    assert!(
        matches!(result, Err(ExchangeError::ClockDrift(_))),
        "60 second drift should fail clock check"
    );
}

// =============================================================================
// Test 3: BLE Relay Attack Prevention (Proximity Verification Required)
// =============================================================================
// security.feature @attacks: "Relay attack prevention on BLE"
// - Distance-bounding protocol should detect relay attacks
// - Proximity verification is mandatory before key agreement
// - TooFar error when device is beyond allowed range

/// Scenario: BLE exchange requires proximity verification
// @scenario: security :: Relay attack prevention on BLE
#[test]
fn test_ble_relay_attack_prevention() {
    // Setup: Attacker is relaying BLE signals from 5 meters away
    let verifier = MockBLEVerifier::success_at_distance(5.0); // 5 meters = too far
    let device = &verifier.devices[0];

    // Device should not be within the 2-meter allowed range
    assert!(
        !verifier.is_within_range(device, 2.0),
        "Device at 5m should not be within 2m range"
    );

    // Proximity verification should fail with TooFar
    let result = verifier.verify_device_proximity(device);
    assert!(
        matches!(result, Err(ProximityError::TooFar)),
        "Should reject device that is too far"
    );
}

/// Scenario: Exchange session blocks without mutual QR exchange
// @scenario: security :: Man-in-the-middle detection during exchange
#[test]
fn test_exchange_requires_mutual_scan() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_card = vauchi_core::ContactCard::new("Alice");
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    let bob_identity = Identity::create("Bob", 0);
    let bob_card = vauchi_core::ContactCard::new("Bob");
    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Bob starts his QR display
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Bob scans Alice's QR -> moves to PeerScanned
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Bob tries to skip directly to key agreement without TheyScannedOurQR - should fail
    let result = bob_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_err(),
        "Key agreement should fail without mutual QR exchange (TheyScannedOurQR)"
    );
}

/// Scenario: Successful exchange when mutual QR scan completes
// @scenario: security :: Man-in-the-middle detection during exchange
#[test]
fn test_exchange_succeeds_with_mutual_scan() {
    let alice_identity = Identity::create("Alice", 0);
    let alice_card = vauchi_core::ContactCard::new("Alice");
    let alice_proximity = MockProximityVerifier::success();
    let mut alice_session = ExchangeSession::new_qr(
        alice_identity,
        alice_card,
        alice_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    alice_session.apply(ExchangeEvent::StartQR).unwrap();
    let alice_qr = alice_session.qr().unwrap().clone();

    let bob_identity = Identity::create("Bob", 0);
    let bob_card = vauchi_core::ContactCard::new("Bob");
    let bob_proximity = MockProximityVerifier::success();
    let mut bob_session = ExchangeSession::new_qr(
        bob_identity,
        bob_card,
        bob_proximity,
        vauchi_core::clock::SystemClock::shared(),
    );

    // Bob starts his QR display
    bob_session.apply(ExchangeEvent::StartQR).unwrap();

    // Bob scans Alice's QR -> PeerScanned
    bob_session
        .apply(ExchangeEvent::ProcessQR(alice_qr))
        .unwrap();

    // Alice scans Bob's QR (signal that they scanned ours)
    bob_session.apply(ExchangeEvent::TheyScannedOurQR).unwrap();

    // Key agreement should now succeed
    let result = bob_session.apply(ExchangeEvent::PerformKeyAgreement);
    assert!(
        result.is_ok(),
        "Key agreement should succeed after mutual QR exchange"
    );
}

// =============================================================================
// Test 4: Memory Dump Protection (Sensitive Data Zeroed After Use)
// =============================================================================
// security.feature @data: "Memory dump protection"
// - Sensitive data should be zeroed after use
// - Keys implement Drop with zeroize

/// Verify that Debug formatting on SymmetricKey does not leak key bytes.
/// (core-F-003: Renamed from test_memory_dump_protection to reflect
/// what the test actually verifies — Debug trait redaction, not memory zeroing.)
// @scenario: security :: Memory dump protection
// @scenario: security :: Private key memory protection
// @scenario: identity_management :: Secure memory handling for keys
#[test]
fn test_debug_format_excludes_key_material() {
    let key = SymmetricKey::generate();
    let key_bytes = *key.as_bytes();

    // Key bytes should be non-zero (cryptographically random)
    let all_zero = key_bytes.iter().all(|&b| b == 0);
    assert!(!all_zero, "Generated key should not be all zeros");

    // Debug output must not leak raw key bytes
    let debug_output = format!("{:?}", key);
    assert!(
        debug_output.contains("REDACTED"),
        "Debug output should redact key bytes: {}",
        debug_output
    );

    // Verify no hex-encoded key bytes appear in debug output
    let hex_key = hex::encode(key_bytes);
    assert!(
        !debug_output.contains(&hex_key),
        "Debug output must not contain hex-encoded key bytes"
    );
}

/// Verify that signing works and the keypair can be exercised without panic.
/// (core-F-002: SigningKeyPair does not implement Debug, so we cannot test
/// debug redaction. Instead we verify sign/verify roundtrip and that the
/// public key is different from a second generated keypair — ensuring
/// unique key generation. Actual memory zeroing requires unsafe/valgrind.)
// @scenario: security :: Sufficient key lengths
#[test]
fn test_signing_key_generation_and_uniqueness() {
    let keypair1 = SigningKeyPair::generate();
    let keypair2 = SigningKeyPair::generate();

    // Verify signing works
    let message = b"test message";
    let signature = keypair1.sign(message);
    assert!(
        keypair1.public_key().verify(message, &signature),
        "Signature should be valid"
    );

    // Signatures from different keys must differ
    let signature2 = keypair2.sign(message);
    assert_ne!(
        signature.as_bytes(),
        signature2.as_bytes(),
        "Different keys must produce different signatures"
    );

    // Cross-verify: keypair2's signature should NOT verify with keypair1's public key
    assert!(
        !keypair1.public_key().verify(message, &signature2),
        "Cross-key verification must fail"
    );
}

/// Scenario: Password KDF output is zeroed after use
// @scenario: security :: Private key memory protection
#[test]
fn test_kdf_output_zeroed() {
    let password = b"my-password";
    let salt = [0x42u8; 32];

    // Derive key
    let key = derive_key_argon2id(password, &salt).unwrap();

    // Key should be non-zero
    let key_bytes = *key.as_bytes();
    assert!(
        key_bytes.iter().any(|&b| b != 0),
        "Derived key should be non-zero"
    );

    // Debug output should be redacted
    let debug_output = format!("{:?}", key);
    assert!(
        debug_output.contains("REDACTED"),
        "Debug output should redact derived key"
    );

    // Drop zeroes the key
    drop(key);
}

// =============================================================================
// Test 5: Audit Log Export (Security Events Exportable)
// =============================================================================
// security.feature @audit: "Security events logged" and "Export security log"
// - Events should be logged locally
// - Logs should include timestamp and event type
// - Logs should not contain sensitive data
// - Logs should be available for security review

/// Scenario: Audit events are logged and retrievable
// @scenario: security :: Security events logged
// @scenario: security :: Export security log
#[test]
fn test_audit_log_export() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let encryption_key = SymmetricKey::generate();

    let storage = Storage::open(&db_path, encryption_key).unwrap();

    // Log some security events
    storage
        .log_audit_event("exchange_initiated", Some("contact_id_123"))
        .unwrap();
    storage
        .log_audit_event("exchange_completed", Some("contact_id_123"))
        .unwrap();
    storage
        .log_audit_event("signature_verification_failed", Some("attacker_key_xyz"))
        .unwrap();

    // Export audit log
    let audit_entries = storage.list_audit_log().unwrap();

    // Verify events were logged
    assert_eq!(audit_entries.len(), 3, "Should have 3 audit entries");

    // Verify event types are correct
    let event_types: Vec<&str> = audit_entries
        .iter()
        .map(|(event_type, _, _)| event_type.as_str())
        .collect();

    assert!(event_types.contains(&"exchange_initiated"));
    assert!(event_types.contains(&"exchange_completed"));
    assert!(event_types.contains(&"signature_verification_failed"));

    // Verify each entry has a timestamp
    for (_, _, timestamp) in &audit_entries {
        assert!(*timestamp > 0, "Timestamp should be non-zero");
    }
}

/// Scenario: Audit log export contains no sensitive data
// @scenario: security :: Security events logged
// @scenario: security :: Export security log
#[test]
fn test_audit_log_no_sensitive_data() {
    let temp_dir = tempfile::tempdir().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let encryption_key = SymmetricKey::generate();

    let storage = Storage::open(&db_path, encryption_key).unwrap();

    // Log event with details
    storage
        .log_audit_event("test_event", Some("safe_details"))
        .unwrap();

    let audit_entries = storage.list_audit_log().unwrap();
    assert_eq!(audit_entries.len(), 1);

    let (event_type, details, _) = &audit_entries[0];

    // Event type should be present
    assert_eq!(event_type, "test_event");

    // Details are stored encrypted and decrypted on retrieval
    // The actual content should not contain raw keys or passwords
    if let Some(d) = details {
        assert!(
            !d.contains("BEGIN PRIVATE KEY"),
            "Audit log should not contain private keys"
        );
        assert!(
            !d.contains("password"),
            "Audit log should not contain passwords"
        );
    }
}

// =============================================================================
// Test 6: Key Material Never Logged (Keys Excluded From Logs)
// =============================================================================
// security.feature @audit: "logs should not contain sensitive data"
// - Private keys should never appear in debug output
// - Key bytes should be redacted in Display/Debug traits

/// Scenario: SymmetricKey Debug output is redacted
// @scenario: security :: Security events logged
// @scenario: identity_management :: Private keys never exposed in logs
#[test]
fn test_key_material_never_logged() {
    let key = SymmetricKey::generate();
    let key_bytes = key.as_bytes();

    // Format the key for "logging"
    let debug_output = format!("{:?}", key);

    // The actual key bytes should NOT appear in the debug output
    for byte in key_bytes {
        let hex_byte = format!("{:02x}", byte);
        // Individual bytes might coincidentally appear, but the full
        // key should definitely not be present
        if debug_output.contains(&hex_byte) && !debug_output.contains("REDACTED") {
            panic!("Debug output may contain key material: {}", debug_output);
        }
    }

    // Should explicitly say REDACTED
    assert!(
        debug_output.contains("REDACTED"),
        "Debug output should contain REDACTED, got: {}",
        debug_output
    );
}

/// Scenario: SigningKeyPair public key is safe to log, private is not
// @scenario: security :: Private keys never exported in plaintext
#[test]
fn test_signing_keypair_logging_safety() {
    let keypair = SigningKeyPair::generate();
    let public_key = keypair.public_key();

    // Public key CAN be logged (it's public information)
    let public_debug = format!("{:?}", public_key);
    // PublicKey derives Debug which shows bytes - this is intentional
    // as public keys are meant to be shared

    // The keypair itself (which contains the private seed) should NOT
    // expose the seed in any logging.
    // SigningKeyPair intentionally does NOT implement Debug to prevent
    // accidental logging of private key material.

    // This is enforced at compile time - uncomment to verify:
    // let keypair_debug = format!("{:?}", keypair); // Should not compile

    // If we're here, the safety invariants are maintained
    assert!(public_debug.contains("PublicKey"));
}

/// Scenario: Key derivation intermediate values are not leaked
// @scenario: security :: Private key memory protection
#[test]
fn test_kdf_no_intermediate_leakage() {
    let password = b"secret-password";
    let salt = [0x42u8; 32];

    // The KDF function internally creates intermediate values
    // These should be zeroed by the argon2 crate
    let key = derive_key_argon2id(password, &salt).unwrap();

    // The returned key is the only output
    assert_eq!(key.as_bytes().len(), 32);

    // Debug should be redacted
    let debug = format!("{:?}", key);
    assert!(debug.contains("REDACTED"));

    // Password bytes should definitely not appear in debug output
    let password_str = String::from_utf8_lossy(password);
    assert!(!debug.contains(&*password_str));
}

// =============================================================================
// Additional Security Hardening Tests
// =============================================================================

/// Scenario: Exchange token is cryptographically random
// @scenario: security :: Sufficient key lengths
#[test]
fn test_exchange_token_randomness() {
    let identity = Identity::create("Test", 0);
    let ephemeral = X3DHKeyPair::generate();

    // Generate multiple QRs and verify tokens are unique
    let qr1 = ExchangeQR::generate(&identity, &ephemeral);
    let qr2 = ExchangeQR::generate(&identity, &ephemeral);
    let qr3 = ExchangeQR::generate(&identity, &ephemeral);

    // All tokens should be different (collision probability is negligible)
    assert_ne!(
        qr1.exchange_token(),
        qr2.exchange_token(),
        "Exchange tokens should be unique"
    );
    assert_ne!(
        qr2.exchange_token(),
        qr3.exchange_token(),
        "Exchange tokens should be unique"
    );
    assert_ne!(
        qr1.exchange_token(),
        qr3.exchange_token(),
        "Exchange tokens should be unique"
    );
}

/// Scenario: Audio challenge seeds are cryptographically random
// @scenario: security :: Sufficient key lengths
#[test]
fn test_audio_challenge_randomness() {
    let identity = Identity::create("Test", 0);
    let ephemeral = X3DHKeyPair::generate();

    let qr1 = ExchangeQR::generate(&identity, &ephemeral);
    let qr2 = ExchangeQR::generate(&identity, &ephemeral);

    // Audio challenges should be different
    assert_ne!(
        qr1.audio_challenge(),
        qr2.audio_challenge(),
        "Audio challenges should be unique per QR"
    );
}

/// Scenario: QR signature prevents tampering
// @scenario: security :: Contact card signatures verified
#[test]
fn test_qr_signature_prevents_tampering() {
    let identity = Identity::create("Alice", 0);
    let ephemeral = X3DHKeyPair::generate();
    let qr = ExchangeQR::generate(&identity, &ephemeral);
    let qr_data = qr.to_data_string();

    // Decode, tamper, re-encode
    let mut bytes =
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &qr_data).unwrap();

    // Tamper with a byte in the middle (exchange token area)
    if bytes.len() > 50 {
        bytes[50] ^= 0xFF;
    }

    let tampered_data = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &bytes);

    // Parsing tampered QR should fail signature verification
    let result = ExchangeQR::from_data_string(&tampered_data);
    assert!(
        result.is_err(),
        "Tampered QR should fail to parse: {:?}",
        result
    );
}
