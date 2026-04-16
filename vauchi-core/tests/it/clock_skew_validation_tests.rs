// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Platform Edge Cases: Clock Skew Validation Tests
//!
//! Feature file: features/platform_edge_cases.feature @time @clock-skew
//! Tests for configurable clock skew tolerance and ReplayDetector persistence.

use crate::common;

use vauchi_core::crypto::SymmetricKey;
use vauchi_core::storage::Storage;
use vauchi_core::sync::ReplayDetector;

// ============================================================
// Configurable Clock Skew Tolerance Tests
// ============================================================

// @internal
#[test]
fn test_default_tolerance_rejects_large_skew() {
    let mut detector = ReplayDetector::default_tolerance(); // 60s
    let now = current_ts();

    // A message from 2 minutes ago should be accepted (within tolerance)
    let nonce_ok = [0x01u8; 32];
    assert!(
        detector.check_replay("contact-1", &nonce_ok, now),
        "Current timestamp should be accepted"
    );

    // Now try a message much older than tolerance
    let nonce_old = [0x02u8; 32];
    // Set last_ts to now, then try with timestamp 2 minutes before
    let accepted = detector.check_replay("contact-1", &nonce_old, now - 121);
    assert!(
        !accepted,
        "Message 121s behind last accepted (tolerance=60s) should be rejected"
    );
}

// @internal
#[test]
fn test_high_tolerance_accepts_moderate_skew() {
    let mut detector = ReplayDetector::new(3600); // 1 hour tolerance
    let now = current_ts();

    // Record a recent message
    let nonce1 = [0x01u8; 32];
    assert!(detector.check_replay("contact-1", &nonce1, now));

    // A message from 30 minutes ago should be accepted with 1h tolerance
    let nonce2 = [0x02u8; 32];
    let accepted = detector.check_replay("contact-1", &nonce2, now - 1800);
    assert!(
        accepted,
        "30-min-old message should be accepted with 1h tolerance"
    );
}

// @internal
#[test]
fn test_high_tolerance_rejects_beyond_window() {
    let mut detector = ReplayDetector::new(3600); // 1 hour tolerance
    let now = current_ts();

    // Record recent
    let nonce1 = [0x01u8; 32];
    assert!(detector.check_replay("contact-1", &nonce1, now));

    // A message from 2 hours ago should still be rejected
    let nonce2 = [0x02u8; 32];
    let accepted = detector.check_replay("contact-1", &nonce2, now - 7200);
    assert!(
        !accepted,
        "2-hour-old message should be rejected even with 1h tolerance"
    );
}

// @internal
#[test]
fn test_duplicate_nonce_always_rejected() {
    let mut detector = ReplayDetector::new(3600);
    let now = current_ts();
    let nonce = [0x42u8; 32];

    assert!(detector.check_replay("contact-1", &nonce, now));
    assert!(
        !detector.check_replay("contact-1", &nonce, now),
        "Duplicate nonce must always be rejected"
    );
}

// @internal
#[test]
fn test_same_nonce_different_contacts_accepted() {
    let mut detector = ReplayDetector::new(60);
    let now = current_ts();
    let nonce = [0x42u8; 32];

    assert!(detector.check_replay("contact-1", &nonce, now));
    assert!(
        detector.check_replay("contact-2", &nonce, now),
        "Same nonce for different contacts should be accepted"
    );
}

// @internal
#[test]
fn test_prune_removes_old_nonces() {
    let mut detector = ReplayDetector::new(60);

    // Add nonces at different timestamps
    let nonce1 = [0x01u8; 32];
    let nonce2 = [0x02u8; 32];
    let nonce3 = [0x03u8; 32];

    detector.check_replay("old-contact", &nonce1, 1000);
    detector.check_replay("recent-contact", &nonce2, 5000);
    detector.check_replay("recent-contact", &nonce3, 5001);

    // Prune everything before 3000
    detector.prune_before(3000);

    // old-contact nonces should be gone — replaying should succeed
    let nonce1_replay = [0x01u8; 32];
    assert!(
        detector.check_replay("old-contact", &nonce1_replay, 5002),
        "Pruned nonce should be accepted again"
    );

    // recent-contact nonces should still be tracked
    let nonce2_replay = [0x02u8; 32];
    assert!(
        !detector.check_replay("recent-contact", &nonce2_replay, 5003),
        "Non-pruned nonce should still be rejected"
    );
}

// ============================================================
// ReplayDetector Persistence Tests
// ============================================================

// @internal
#[test]
fn test_replay_nonce_persisted_to_storage() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Save a nonce
    let nonce = [0xABu8; 32];
    storage
        .save_replay_nonce("contact-001", &nonce, 1000)
        .unwrap();

    // Load nonces for this contact
    let nonces = storage.load_replay_nonces("contact-001").unwrap();
    assert_eq!(nonces.len(), 1);
    assert_eq!(nonces[0].0, nonce);
    assert_eq!(nonces[0].1, 1000);
}

// @internal
#[test]
fn test_replay_nonces_multiple_contacts() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let nonce_a = [0x01u8; 32];
    let nonce_b = [0x02u8; 32];

    storage
        .save_replay_nonce("contact-a", &nonce_a, 1000)
        .unwrap();
    storage
        .save_replay_nonce("contact-b", &nonce_b, 2000)
        .unwrap();

    let nonces_a = storage.load_replay_nonces("contact-a").unwrap();
    let nonces_b = storage.load_replay_nonces("contact-b").unwrap();
    assert_eq!(nonces_a.len(), 1);
    assert_eq!(nonces_b.len(), 1);
}

// @internal
#[test]
fn test_replay_nonces_cleanup_old() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    let old_nonce = [0x01u8; 32];
    let recent_nonce = [0x02u8; 32];

    storage
        .save_replay_nonce("contact-1", &old_nonce, 1000)
        .unwrap();
    storage
        .save_replay_nonce("contact-1", &recent_nonce, 5000)
        .unwrap();

    // Cleanup nonces older than 3000
    let removed = storage.cleanup_replay_nonces(3000).unwrap();
    assert_eq!(removed, 1, "Should remove 1 old nonce");

    let remaining = storage.load_replay_nonces("contact-1").unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].0, recent_nonce);
}

// @internal
#[test]
fn test_replay_detector_roundtrip_via_storage() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();

    // Simulate: detector accepts a nonce, then persists it
    let mut detector = ReplayDetector::new(3600);
    let nonce = [0x42u8; 32];
    let timestamp = 5000u64;

    assert!(detector.check_replay("contact-x", &nonce, timestamp));

    // Persist to storage
    storage
        .save_replay_nonce("contact-x", &nonce, timestamp)
        .unwrap();

    // Simulate restart: create new detector and load from storage
    let mut detector2 = ReplayDetector::new(3600);
    let nonces = storage.load_replay_nonces("contact-x").unwrap();
    for (stored_nonce, stored_ts) in &nonces {
        // Pre-populate the detector from storage
        detector2.check_replay("contact-x", stored_nonce, *stored_ts);
    }

    // Now the duplicate nonce should be rejected
    assert!(
        !detector2.check_replay("contact-x", &nonce, timestamp),
        "Nonce loaded from storage should prevent replay"
    );
}

// @internal
#[test]
fn test_duplicate_nonce_insert_is_idempotent() {
    let storage = Storage::in_memory(SymmetricKey::generate()).unwrap();
    let nonce = [0xFFu8; 32];

    storage
        .save_replay_nonce("contact-1", &nonce, 1000)
        .unwrap();
    // Inserting same nonce again should not fail (OR IGNORE)
    storage
        .save_replay_nonce("contact-1", &nonce, 1000)
        .unwrap();

    let nonces = storage.load_replay_nonces("contact-1").unwrap();
    assert_eq!(nonces.len(), 1, "Duplicate insert should be idempotent");
}

// ============================================================
// Helpers
// ============================================================

fn current_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
