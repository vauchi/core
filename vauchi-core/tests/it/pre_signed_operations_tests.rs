// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for pre-signed operations.
//!
//! Traces to features/pre_signed_operations.feature:
//!   - "Refresh pre-signed messages periodically"
//!   - "Refresh generates new purge token for replay prevention"
//!   - (revocation of pre-signed operations — DP-2 compliance)
//!   - "Pre-signed messages stored unencrypted" (offline storage)

use vauchi_core::api::PreSignedShredMessages;
use vauchi_core::crypto::signing::{PublicKey as CryptoPublicKey, Signature as CryptoSignature};
use vauchi_core::identity::Identity;

/// Helper: verify an Ed25519 signature using the crypto module.
fn verify_ed25519(public_key: &[u8; 32], message: &[u8], signature: &[u8]) -> bool {
    let pk = CryptoPublicKey::from_bytes(*public_key);
    let sig_bytes: [u8; 64] = signature.try_into().expect("signature should be 64 bytes");
    let sig = CryptoSignature::from_bytes(sig_bytes);
    pk.verify(message, &sig)
}

// === Refresh Tests ===
// Traces to: "Refresh pre-signed messages periodically"
//            "Refresh generates new purge token for replay prevention"

/// Test that refresh generates valid messages before expiration.
///
/// Traces to: Scenario "Refresh pre-signed messages periodically"
// @scenario: pre_signed_operations :: Refresh pre-signed messages periodically
// @scenario: pre_signed_operations :: Pre-signed messages created at identity setup
#[test]
fn test_pre_signed_message_refresh() {
    // Given pre-signed messages were generated
    let identity = Identity::create("Alice", 0);
    let original = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    let original_refreshed_at = original.refreshed_at;

    // When the refresh mechanism runs (before expiration)
    let refreshed = PreSignedShredMessages::refresh(&identity, 1_700_000_000);

    assert!(
        refreshed.refreshed_at >= original_refreshed_at,
        "Refreshed timestamp should be >= original"
    );

    let purge = &refreshed.purge_request;
    let mut message = Vec::with_capacity(32 + 32 + 8);
    message.extend_from_slice(&purge.public_key);
    message.extend_from_slice(&purge.purge_token);
    message.extend_from_slice(&purge.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(&purge.public_key, &message, &purge.signature),
        "Refreshed purge request signature should be valid"
    );

    let notice = &refreshed.deletion_notice;
    let stage_byte = 1u8; // Confirmed
    let mut notice_message = Vec::with_capacity(32 + 1 + 8);
    notice_message.extend_from_slice(notice.public_key.as_bytes());
    notice_message.push(stage_byte);
    notice_message.extend_from_slice(&notice.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(
            notice.public_key.as_bytes(),
            &notice_message,
            &notice.signature
        ),
        "Refreshed deletion notice signature should be valid"
    );
}

/// Test that refreshed messages have different purge tokens for replay prevention.
///
/// Traces to: Scenario "Refresh generates new purge token for replay prevention"
// @scenario: pre_signed_operations :: Refresh generates new purge token for replay prevention
#[test]
fn test_pre_signed_refresh_generates_new_purge_token() {
    // Given I have existing pre-signed messages with purge token A
    let identity = Identity::create("Bob", 0);
    let msgs_a = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    let token_a = msgs_a.purge_request.purge_token;

    // When I refresh the pre-signed messages
    let msgs_b = PreSignedShredMessages::refresh(&identity, 1_700_000_000);
    let token_b = msgs_b.purge_request.purge_token;

    assert_ne!(
        token_a, token_b,
        "Refreshed purge token should be different for replay prevention"
    );

    // And both tokens should be 32 bytes (proper random tokens)
    assert_eq!(token_a.len(), 32);
    assert_eq!(token_b.len(), 32);
}

// === Token Rotation Tests ===
// Traces to: "Refresh generates new purge token for replay prevention"

/// Test that purge token changes when identity keys are used for signing.
/// Even with the same identity, each generation produces a unique token.
///
/// Traces to: Scenario "Refresh generates new purge token for replay prevention"
// @scenario: pre_signed_operations :: Refresh generates new purge token for replay prevention
#[test]
fn test_purge_token_rotation() {
    let identity = Identity::create("Charlie", 0);

    let msgs1 = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    let msgs2 = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    let msgs3 = PreSignedShredMessages::generate(&identity, 1_700_000_000);

    // Each purge token should be unique (random)
    assert_ne!(
        msgs1.purge_request.purge_token, msgs2.purge_request.purge_token,
        "Purge tokens should differ between generations"
    );
    assert_ne!(
        msgs2.purge_request.purge_token, msgs3.purge_request.purge_token,
        "Purge tokens should differ between generations"
    );
    assert_ne!(
        msgs1.purge_request.purge_token, msgs3.purge_request.purge_token,
        "Purge tokens should differ between generations"
    );

    for msgs in [&msgs1, &msgs2, &msgs3] {
        let purge = &msgs.purge_request;
        assert_eq!(
            purge.public_key,
            *identity.signing_public_key(),
            "Public key should match identity"
        );
        assert_eq!(purge.signature.len(), 64, "Signature should be 64 bytes");
    }
}

/// Test that token rotation maintains signature validity for each token.
// @scenario: pre_signed_operations :: Refresh generates new purge token for replay prevention
// @scenario: pre_signed_operations :: Pre-signed purge request has valid Ed25519 signature
#[test]
fn test_purge_token_rotation_maintains_signature_validity() {
    let identity = Identity::create("Dave", 0);

    let versions: Vec<PreSignedShredMessages> = (0..5)
        .map(|_| PreSignedShredMessages::generate(&identity, 1_700_000_000))
        .collect();

    for msgs in &versions {
        let purge = &msgs.purge_request;

        let mut message = Vec::with_capacity(32 + 32 + 8);
        message.extend_from_slice(&purge.public_key);
        message.extend_from_slice(&purge.purge_token);
        message.extend_from_slice(&purge.timestamp.to_be_bytes());

        assert!(
            verify_ed25519(&purge.public_key, &message, &purge.signature),
            "Each version's signature should be valid for its token"
        );
    }

    if versions.len() >= 2 {
        let msgs_a = &versions[0];
        let msgs_b = &versions[1];

        // Try to verify A's signature with B's token (should fail)
        let mut mixed_message = Vec::with_capacity(32 + 32 + 8);
        mixed_message.extend_from_slice(&msgs_a.purge_request.public_key);
        mixed_message.extend_from_slice(&msgs_b.purge_request.purge_token); // Wrong token!
        mixed_message.extend_from_slice(&msgs_a.purge_request.timestamp.to_be_bytes());

        assert!(
            !verify_ed25519(
                &msgs_a.purge_request.public_key,
                &mixed_message,
                &msgs_a.purge_request.signature
            ),
            "Signature should NOT verify with wrong token"
        );
    }
}

// === Revocation Tests ===
// Pre-signed operations cannot themselves be revoked once generated,
// but they are used for revocation notifications.

/// Test that pre-signed messages can be used to revoke/notify after key destruction.
/// This is the core DP-2 (sign-before-destroy) principle.
///
/// Traces to: Scenario "Pre-signed messages remain valid after key destruction"
// @scenario: pre_signed_operations :: Pre-signed messages remain valid after key destruction
// @scenario: pre_signed_operations :: Deletion notice stages
#[test]
fn test_pre_signed_revocation() {
    // Given I have pre-signed messages
    let identity = Identity::create("Eve", 0);
    let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);

    // Store the public key for later verification
    let public_key = *identity.signing_public_key();

    // When the identity is "destroyed" (we drop it)
    drop(identity);

    let notice = &msgs.deletion_notice;
    let stage_byte = 1u8; // Confirmed
    let mut notice_message = Vec::with_capacity(32 + 1 + 8);
    notice_message.extend_from_slice(notice.public_key.as_bytes());
    notice_message.push(stage_byte);
    notice_message.extend_from_slice(&notice.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(&public_key, &notice_message, &notice.signature),
        "Deletion notice should verify after identity destruction"
    );

    // And the pre-signed purge request can still be verified
    let purge = &msgs.purge_request;
    let mut purge_message = Vec::with_capacity(32 + 32 + 8);
    purge_message.extend_from_slice(&purge.public_key);
    purge_message.extend_from_slice(&purge.purge_token);
    purge_message.extend_from_slice(&purge.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(&public_key, &purge_message, &purge.signature),
        "Purge request should verify after identity destruction"
    );
}

/// Test that revocation via pre-signed messages includes correct public key.
// @scenario: pre_signed_operations :: Pre-signed purge request has valid Ed25519 signature
// @scenario: pre_signed_operations :: Pre-signed purge request contains required fields
// @scenario: pre_signed_operations :: Pre-signed deletion notice contains required fields
#[test]
fn test_pre_signed_revocation_includes_public_key() {
    let identity = Identity::create("Frank", 0);
    let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);

    assert_eq!(
        msgs.deletion_notice.public_key,
        *identity.signing_public_key(),
        "Deletion notice should include identity's public key"
    );
    assert_eq!(
        msgs.purge_request.public_key,
        *identity.signing_public_key(),
        "Purge request should include identity's public key"
    );
}

// === Offline Storage Tests ===
// Traces to: "Pre-signed messages stored unencrypted"

/// Test that pre-signed messages work without network.
/// They can be saved and loaded from disk without any network calls.
///
/// Traces to: Scenario "Pre-signed messages stored unencrypted"
// @scenario: pre_signed_operations :: Pre-signed messages stored unencrypted
#[test]
fn test_pre_signed_offline_storage() {
    let dir = tempfile::tempdir().unwrap();

    // Given I create pre-signed messages offline
    let identity = Identity::create("Grace", 0);
    let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);

    // When I save them to disk
    msgs.save(dir.path()).unwrap();

    let loaded = PreSignedShredMessages::load(dir.path()).unwrap();

    assert_eq!(loaded.refreshed_at, msgs.refreshed_at);
    assert_eq!(
        loaded.deletion_notice.public_key,
        msgs.deletion_notice.public_key
    );
    assert_eq!(
        loaded.deletion_notice.timestamp,
        msgs.deletion_notice.timestamp
    );
    assert_eq!(
        loaded.deletion_notice.signature,
        msgs.deletion_notice.signature
    );
    assert_eq!(
        loaded.purge_request.public_key,
        msgs.purge_request.public_key
    );
    assert_eq!(
        loaded.purge_request.purge_token,
        msgs.purge_request.purge_token
    );
    assert_eq!(loaded.purge_request.signature, msgs.purge_request.signature);
}

/// Test that offline-stored messages can be loaded and used after app restart.
///
/// Traces to: Scenario "Pre-signed messages survive app restarts"
// @scenario: pre_signed_operations :: Pre-signed messages survive app restarts
#[test]
fn test_pre_signed_offline_storage_survives_restart() {
    let dir = tempfile::tempdir().unwrap();

    // Create and save pre-signed messages (simulating first app run)
    let public_key = {
        let identity = Identity::create("Henry", 0);
        let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);
        msgs.save(dir.path()).unwrap();
        *identity.signing_public_key()
    };

    // Identity is now dropped (simulating app restart)

    // Load the messages (simulating second app run)
    let loaded = PreSignedShredMessages::load(dir.path()).unwrap();

    let purge = &loaded.purge_request;
    let mut message = Vec::with_capacity(32 + 32 + 8);
    message.extend_from_slice(&purge.public_key);
    message.extend_from_slice(&purge.purge_token);
    message.extend_from_slice(&purge.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(&public_key, &message, &purge.signature),
        "Loaded messages should have valid signatures"
    );
}

/// Test that offline storage file is readable without decryption.
/// Per DP-3, pre-signed messages are stored unencrypted.
///
/// Traces to: Scenario "Pre-signed messages stored unencrypted"
// @scenario: pre_signed_operations :: Pre-signed messages stored unencrypted
#[test]
fn test_pre_signed_offline_storage_unencrypted() {
    let dir = tempfile::tempdir().unwrap();
    let identity = Identity::create("Ivy", 0);
    let msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    msgs.save(dir.path()).unwrap();

    let path = PreSignedShredMessages::file_path(dir.path());
    let raw_bytes = std::fs::read(&path).unwrap();

    // Should be directly deserializable (no decryption needed)
    let loaded: PreSignedShredMessages = postcard::from_bytes(&raw_bytes)
        .expect("Pre-signed messages should be readable without decryption");

    assert_eq!(
        loaded.purge_request.public_key,
        msgs.purge_request.public_key
    );
}

/// Test that pre-signed messages work in airplane mode scenario.
/// Simulates a user preparing messages before going offline.
// @scenario: pre_signed_operations :: Pre-signed messages stored unencrypted
#[test]
fn test_pre_signed_offline_airplane_mode() {
    let dir = tempfile::tempdir().unwrap();

    // Step 1: User generates pre-signed messages while online
    let identity = Identity::create("Jake", 0);
    let original_msgs = PreSignedShredMessages::generate(&identity, 1_700_000_000);
    original_msgs.save(dir.path()).unwrap();

    // Step 2: User goes offline (simulated by not having network)

    // Step 3: User needs to shred while offline
    let loaded = PreSignedShredMessages::load(dir.path()).unwrap();

    // Messages should be ready to use (valid signatures)
    let purge = &loaded.purge_request;
    let mut message = Vec::with_capacity(32 + 32 + 8);
    message.extend_from_slice(&purge.public_key);
    message.extend_from_slice(&purge.purge_token);
    message.extend_from_slice(&purge.timestamp.to_be_bytes());

    assert!(
        verify_ed25519(&purge.public_key, &message, &purge.signature),
        "Pre-signed messages should work in airplane mode"
    );

    // Step 4: When user comes back online, messages can be sent
    // (Network layer tested elsewhere, here we just verify messages are usable)
}

// === Edge Cases ===

/// Test refresh with multiple identities doesn't cross-contaminate.
// @scenario: pre_signed_operations :: Pre-signed purge request has valid Ed25519 signature
#[test]
fn test_pre_signed_refresh_isolation() {
    let alice = Identity::create("Alice", 0);
    let bob = Identity::create("Bob", 0);

    let alice_msgs = PreSignedShredMessages::generate(&alice, 1_700_000_000);
    let bob_msgs = PreSignedShredMessages::generate(&bob, 1_700_000_000);

    assert_eq!(
        alice_msgs.purge_request.public_key,
        *alice.signing_public_key()
    );
    assert_eq!(
        alice_msgs.deletion_notice.public_key,
        *alice.signing_public_key()
    );

    assert_eq!(bob_msgs.purge_request.public_key, *bob.signing_public_key());
    assert_eq!(
        bob_msgs.deletion_notice.public_key,
        *bob.signing_public_key()
    );

    assert_ne!(
        alice_msgs.purge_request.public_key,
        bob_msgs.purge_request.public_key
    );
}

/// Test that timestamps are reasonable (not in the past or far future).
// @scenario: pre_signed_operations :: Refresh pre-signed messages periodically
#[test]
fn test_pre_signed_timestamp_sanity() {
    let identity = Identity::create("Kate", 0);
    // Caller-controlled timestamp is stamped verbatim — the old
    // `SystemTime::now()` ± 60s sanity check has been replaced with a
    // direct equality assertion against the input now.
    let now = 1_700_000_000;
    let msgs = PreSignedShredMessages::generate(&identity, now);

    assert_eq!(msgs.refreshed_at, now);
    assert_eq!(msgs.purge_request.timestamp, now);
    assert_eq!(msgs.deletion_notice.timestamp, now);
}
