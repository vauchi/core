// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for network::anonymous (anonymous sender IDs)

use vauchi_core::network::anonymous::{
    compute_anonymous_id, current_epoch, resolve_sender, AnonymousSender, SenderIndex,
};
use vauchi_core::{Contact, ContactCard, SymmetricKey};

fn make_contact_with_key(name: &str, key: SymmetricKey) -> Contact {
    let pk = *vauchi_core::SigningKeyPair::generate()
        .public_key()
        .as_bytes();
    Contact::from_exchange(pk, ContactCard::new(name), key)
}

// @scenario: anonymous_sender.feature:Anonymous sender ID is deterministic for same key and epoch
#[test]
fn test_compute_deterministic() {
    let key = [0xABu8; 32];
    let epoch = 100;
    let id1 = compute_anonymous_id(&key, epoch);
    let id2 = compute_anonymous_id(&key, epoch);
    assert_eq!(id1, id2);
}

// @scenario: anonymous_sender.feature:Anonymous ID changes each epoch
#[test]
fn test_different_epochs_different_ids() {
    let key = [0xABu8; 32];
    let id1 = compute_anonymous_id(&key, 100);
    let id2 = compute_anonymous_id(&key, 101);
    assert_ne!(id1, id2);
}

// @scenario: anonymous_sender.feature:Different shared keys produce different anonymous IDs
#[test]
fn test_different_keys_different_ids() {
    let key1 = [0xABu8; 32];
    let key2 = [0xCDu8; 32];
    let id1 = compute_anonymous_id(&key1, 100);
    let id2 = compute_anonymous_id(&key2, 100);
    assert_ne!(id1, id2);
}

// @scenario: anonymous_sender.feature:Generate anonymous sender ID for relay message
#[test]
fn test_anonymous_sender_compute() {
    let key = [0x42u8; 32];
    let epoch = 500;
    let sender = AnonymousSender::compute(&key, epoch);
    assert_eq!(sender.epoch, epoch);
    assert_eq!(sender.anonymous_id, compute_anonymous_id(&key, epoch));
}

// @scenario: anonymous_sender.feature:Generate anonymous sender ID for relay message
#[test]
fn test_anonymous_sender_for_current_epoch() {
    let key = [0x42u8; 32];
    let sender = AnonymousSender::for_current_epoch(&key);
    assert_eq!(sender.epoch, current_epoch());
}

// @scenario: anonymous_sender.feature:Epoch boundary handling
#[test]
fn test_current_epoch_is_reasonable() {
    let epoch = current_epoch();
    // Should be > 0 (we're well past UNIX epoch)
    assert!(epoch > 0);
    // In 2026, epoch ~= 1768000000 / 3600 ~= 491111
    assert!(epoch > 400_000);
}

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_resolve_sender_finds_match() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch);

    let result = resolve_sender(&contacts, &anon_id, epoch);
    result.expect("expected Some");
    assert_eq!(result.unwrap().display_name(), "Alice");
}

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_resolve_sender_no_match() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key);
    let contacts = vec![contact];

    let wrong_id = [0u8; 32];
    let result = resolve_sender(&contacts, &wrong_id, 1000);
    assert!(result.is_none());
}

// @scenario: anonymous_sender.feature:Resolution tolerates previous epoch for clock skew
#[test]
fn test_resolve_sender_previous_epoch_tolerance() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Bob", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    // Compute ID for previous epoch
    let prev_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch - 1);

    // Should still resolve when checking current epoch (boundary tolerance)
    let result = resolve_sender(&contacts, &prev_id, epoch);
    result.expect("expected Some");
    assert_eq!(result.unwrap().display_name(), "Bob");
}

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_resolve_sender_empty_contacts() {
    let contacts: Vec<Contact> = vec![];
    let anon_id = [0u8; 32];
    let result = resolve_sender(&contacts, &anon_id, 1000);
    assert!(result.is_none());
}

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_resolve_sender_epoch_zero() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), 0);
    let result = resolve_sender(&contacts, &anon_id, 0);
    result.expect("expected Some");
}

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_resolve_sender_multiple_contacts() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let contacts = vec![
        make_contact_with_key("Alice", key1),
        make_contact_with_key("Bob", key2),
    ];

    let epoch = 500;
    let bob_id = compute_anonymous_id(contacts[1].shared_key().as_bytes(), epoch);

    let result = resolve_sender(&contacts, &bob_id, epoch);
    result.expect("expected Some");
    assert_eq!(result.unwrap().display_name(), "Bob");
}

// ============================================================
// Clock Skew Tolerance Tests
// Traces to: features/anonymous_sender.feature @resolution
// "Resolution tolerates previous epoch for clock skew"
// ============================================================

/// Tests that the resolver tolerates clock skew by accepting messages
/// from the previous epoch. The current implementation accepts epoch N-1
/// when resolving at epoch N, providing ~1 hour of tolerance.
///
/// Note: ±5 minute tolerance within an epoch is implicit since epochs are
/// hourly boundaries. Messages sent within 5 minutes of each other in the
/// same epoch will always have matching IDs.
// @scenario: anonymous_sender.feature:Resolution tolerates previous epoch for clock skew
#[test]
fn test_clock_skew_tolerance() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let current_epoch = 1000;

    // Sender's clock is behind (previous epoch)
    let sender_epoch = current_epoch - 1;
    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), sender_epoch);

    // Receiver resolves at current epoch - should find Alice due to tolerance
    let result = resolve_sender(&contacts, &anon_id, current_epoch);
    assert!(
        result.is_some(),
        "Should resolve sender from previous epoch"
    );
    assert_eq!(result.unwrap().display_name(), "Alice");

    // Two epochs back should NOT resolve (beyond tolerance window)
    let stale_epoch = current_epoch - 2;
    let stale_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), stale_epoch);
    let stale_result = resolve_sender(&contacts, &stale_id, current_epoch);
    assert!(
        stale_result.is_none(),
        "Should NOT resolve sender from 2 epochs ago"
    );
}

// ============================================================
// Epoch Boundary Handling Tests
// Traces to: features/anonymous_sender.feature @epoch
// "Epoch boundary handling"
// ============================================================

/// Tests that messages sent near epoch boundaries are handled correctly.
/// When an epoch transition occurs, the anonymous ID changes, but the
/// resolver tolerates the previous epoch for boundary conditions.
// @scenario: anonymous_sender.feature:Epoch boundary handling
#[test]
fn test_epoch_boundary_handling() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("EdgeCase", key.clone());
    let contacts = vec![contact];

    let epoch_n = 500;
    let epoch_n_plus_1 = 501;

    // Message sent at end of epoch N
    let id_epoch_n = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch_n);

    // Message sent at start of epoch N+1
    let id_epoch_n_plus_1 =
        compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch_n_plus_1);

    // IDs should be different (epoch rotation)
    assert_ne!(
        id_epoch_n, id_epoch_n_plus_1,
        "IDs must differ across epoch boundary"
    );

    // Receiver in epoch N+1 should resolve both:
    // - Message from epoch N (boundary tolerance)
    // - Message from epoch N+1 (current epoch)
    let result_n = resolve_sender(&contacts, &id_epoch_n, epoch_n_plus_1);
    let result_n_plus_1 = resolve_sender(&contacts, &id_epoch_n_plus_1, epoch_n_plus_1);

    assert!(
        result_n.is_some(),
        "Should resolve epoch N message at epoch N+1"
    );
    assert!(
        result_n_plus_1.is_some(),
        "Should resolve epoch N+1 message at epoch N+1"
    );
    assert_eq!(result_n.unwrap().display_name(), "EdgeCase");
    assert_eq!(result_n_plus_1.unwrap().display_name(), "EdgeCase");

    // Receiver in epoch N should NOT resolve epoch N+1 message (future epoch)
    let future_result = resolve_sender(&contacts, &id_epoch_n_plus_1, epoch_n);
    assert!(
        future_result.is_none(),
        "Should NOT resolve future epoch message"
    );
}

// ============================================================
// Sender Unlinkability Tests
// Traces to: features/anonymous_sender.feature @privacy
// "Relay cannot link sender across epochs"
// ============================================================

/// Tests that anonymous IDs from different epochs are cryptographically
/// unlinkable. An observer (e.g., relay) seeing IDs from different epochs
/// cannot determine they originate from the same sender.
// @scenario: anonymous_sender.feature:Relay cannot link sender across epochs
// @scenario: relay_network.feature:Relay cannot correlate sender and recipient
#[test]
fn test_sender_unlinkability() {
    let key = [0x42u8; 32];

    // Collect IDs across 10 consecutive epochs
    let epochs: Vec<u64> = (1000..1010).collect();
    let ids: Vec<[u8; 32]> = epochs
        .iter()
        .map(|&e| compute_anonymous_id(&key, e))
        .collect();

    // All IDs must be unique (no collisions across epochs)
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(
                ids[i], ids[j],
                "IDs for epochs {} and {} must be different",
                epochs[i], epochs[j]
            );
        }
    }

    // IDs should have no obvious correlation (statistical test: check byte variance)
    // A weak derivation might produce similar prefixes/suffixes
    let first_bytes: Vec<u8> = ids.iter().map(|id| id[0]).collect();
    let unique_first_bytes: std::collections::HashSet<_> = first_bytes.iter().collect();
    // With 10 random-ish values, we expect reasonable diversity
    assert!(
        unique_first_bytes.len() >= 5,
        "First bytes should show diversity (got {} unique out of 10)",
        unique_first_bytes.len()
    );
}

/// Tests that the same sender communicating with different contacts
/// produces unlinkable anonymous IDs, even in the same epoch.
// @scenario: anonymous_sender.feature:Relay cannot link sender across epochs
// @scenario: relay_network.feature:Relay cannot correlate sender and recipient
#[test]
fn test_sender_unlinkability_across_contacts() {
    // Simulate shared keys with different contacts
    let key_with_alice = [0xAAu8; 32];
    let key_with_bob = [0xBBu8; 32];
    let key_with_carol = [0xCCu8; 32];

    let epoch = 1000;

    let id_alice = compute_anonymous_id(&key_with_alice, epoch);
    let id_bob = compute_anonymous_id(&key_with_bob, epoch);
    let id_carol = compute_anonymous_id(&key_with_carol, epoch);

    // All three IDs must be different
    assert_ne!(id_alice, id_bob);
    assert_ne!(id_alice, id_carol);
    assert_ne!(id_bob, id_carol);

    // Relay observing these three IDs cannot determine they're from the same sender
    // (This is the privacy property - different shared keys → different IDs)
}

// ============================================================
// Replay Prevention Tests
// Traces to: features/anonymous_sender.feature @resolution
// Note: Core anonymous ID module provides identification, not replay prevention.
// Replay prevention should be implemented at the message processing layer
// using message IDs, timestamps, or sequence numbers.
// ============================================================

/// Tests the foundation for replay prevention: anonymous IDs are epoch-bound.
/// A replayed message from a past epoch will have a stale anonymous ID.
///
/// Full replay prevention requires additional mechanisms at the message layer:
/// - Message ID deduplication
/// - Timestamp validation
/// - Sequence numbers per sender
///
/// This test verifies that the anonymous ID system provides the epoch-binding
/// necessary to detect replay attacks beyond the tolerance window.
// @scenario: anonymous_sender.feature:Anonymous ID changes each epoch
#[test]
fn test_replay_prevention_epoch_binding() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Victim", key.clone());
    let contacts = vec![contact];

    // Attacker captures a message from epoch 1000
    let captured_epoch = 1000;
    let captured_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), captured_epoch);

    // Original message resolves correctly at time of capture
    let original_result = resolve_sender(&contacts, &captured_id, captured_epoch);
    assert!(original_result.is_some(), "Original message should resolve");

    // Attacker replays the message in epoch 1001 (within tolerance - still works)
    let replay_epoch_1 = 1001;
    let replay_result_1 = resolve_sender(&contacts, &captured_id, replay_epoch_1);
    assert!(
        replay_result_1.is_some(),
        "Replay within tolerance window succeeds at ID level"
    );

    // Attacker replays the message in epoch 1002 (beyond tolerance - rejected)
    let replay_epoch_2 = 1002;
    let replay_result_2 = resolve_sender(&contacts, &captured_id, replay_epoch_2);
    assert!(
        replay_result_2.is_none(),
        "Replay beyond tolerance window should fail sender resolution"
    );
}

/// Tests that replay prevention at the message layer would need message-level
/// deduplication. This demonstrates the expected behavior: the same anonymous ID
/// can be used for multiple messages within an epoch, so deduplication must
/// happen at a higher layer using message IDs or sequence numbers.
// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_replay_prevention_same_epoch_requires_message_dedup() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Target", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch);

    // Same anonymous ID resolves successfully multiple times (expected)
    // This is NOT a security flaw - multiple messages in same epoch use same ID
    let first_resolution = resolve_sender(&contacts, &anon_id, epoch);
    let second_resolution = resolve_sender(&contacts, &anon_id, epoch);

    first_resolution.expect("expected Some");
    second_resolution.expect("expected Some");
    assert_eq!(first_resolution.unwrap().display_name(), "Target");
    assert_eq!(second_resolution.unwrap().display_name(), "Target");

    // NOTE: Actual replay prevention must be implemented at the message
    // processing layer, not at the anonymous ID resolution layer.
    // The message layer should maintain a set of seen message IDs and
    // reject duplicates.
}

// ============================================================
// SenderIndex Tests
// Traces to: features/anonymous_sender.feature @resolution
// SenderIndex provides O(1) lookup vs O(n) resolve_sender.
// ============================================================

// @scenario: anonymous_sender.feature:Receiver resolves anonymous sender from contact list
#[test]
fn test_sender_index_build_and_resolve() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let index = SenderIndex::build(&contacts, epoch);

    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch);
    let result = index.resolve(&anon_id);
    assert!(result.is_some(), "SenderIndex should resolve known contact");
    assert_eq!(result.unwrap(), contacts[0].id());
}

// @scenario: anonymous_sender.feature:Resolution tolerates previous epoch for clock skew
#[test]
fn test_sender_index_resolves_previous_epoch() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Bob", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let index = SenderIndex::build(&contacts, epoch);

    // ID from previous epoch should resolve (boundary tolerance)
    let prev_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch - 1);
    let result = index.resolve(&prev_id);
    assert!(
        result.is_some(),
        "SenderIndex should resolve previous epoch ID"
    );
    assert_eq!(result.unwrap(), contacts[0].id());
}

// @scenario: anonymous_sender.feature:Unknown sender cannot be resolved
#[test]
fn test_sender_index_unknown_id_returns_none() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key);
    let contacts = vec![contact];

    let index = SenderIndex::build(&contacts, 1000);
    let unknown_id = [0xFFu8; 32];
    assert!(
        index.resolve(&unknown_id).is_none(),
        "Unknown ID should not resolve"
    );
}

#[test]
fn test_sender_index_empty_contacts() {
    let contacts: Vec<Contact> = vec![];
    let index = SenderIndex::build(&contacts, 1000);
    let any_id = [0x42u8; 32];
    assert!(index.resolve(&any_id).is_none());
}

#[test]
fn test_sender_index_multiple_contacts() {
    let key1 = SymmetricKey::generate();
    let key2 = SymmetricKey::generate();
    let key3 = SymmetricKey::generate();
    let contacts = vec![
        make_contact_with_key("Alice", key1),
        make_contact_with_key("Bob", key2),
        make_contact_with_key("Carol", key3),
    ];

    let epoch = 500;
    let index = SenderIndex::build(&contacts, epoch);

    // Each contact should resolve correctly
    for contact in &contacts {
        let anon_id = compute_anonymous_id(contact.shared_key().as_bytes(), epoch);
        let result = index.resolve(&anon_id);
        assert!(
            result.is_some(),
            "Should resolve {}",
            contact.display_name()
        );
        assert_eq!(result.unwrap(), contact.id());
    }
}

#[test]
fn test_sender_index_stale_detection() {
    let contacts: Vec<Contact> = vec![];
    // Build for a past epoch
    let past_epoch = 1;
    let index = SenderIndex::build(&contacts, past_epoch);
    assert!(index.is_stale(), "Index built for epoch 1 should be stale");
    assert_eq!(index.epoch(), past_epoch);
}

#[test]
fn test_sender_index_epoch_zero() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    // Epoch 0: no previous epoch to check
    let index = SenderIndex::build(&contacts, 0);
    let anon_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), 0);
    let result = index.resolve(&anon_id);
    assert!(result.is_some(), "Should resolve at epoch 0");
}

#[test]
fn test_sender_index_future_epoch_not_resolved() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Alice", key.clone());
    let contacts = vec![contact];

    let epoch = 1000;
    let index = SenderIndex::build(&contacts, epoch);

    // ID from future epoch (epoch + 1) should NOT resolve
    let future_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), epoch + 1);
    assert!(
        index.resolve(&future_id).is_none(),
        "Future epoch ID should not resolve"
    );
}

// ============================================================
// Epoch Calculation Tests
// Traces to: features/anonymous_sender.feature @epoch
// ============================================================

#[test]
fn test_epoch_calculation_formula() {
    // Verify epoch = unix_timestamp / 3600
    // We can't control system time, but we can verify the formula
    // by checking current_epoch is in the right range for 2026
    let epoch = current_epoch();
    // 2026-01-01 00:00:00 UTC = 1767225600 seconds
    // 1767225600 / 3600 = 490896
    // 2026-12-31 23:59:59 UTC = 1798761599 seconds
    // 1798761599 / 3600 = 499656
    assert!(
        (490_000..=510_000).contains(&epoch),
        "Epoch {} should be in 2026 range (490000-510000)",
        epoch
    );
}

// ============================================================
// HKDF Context String Test
// Traces to: features/anonymous_sender.feature @privacy
// "Derivation context prevents cross-protocol confusion"
// ============================================================

// ============================================================
// Anonymous ID Size and Structure Tests
// Traces to: features/anonymous_sender.feature @generation
// "Anonymous ID is 32 bytes"
// ============================================================

// @scenario: anonymous_sender.feature:Anonymous ID is 32 bytes
#[test]
fn test_anonymous_id_is_exactly_32_bytes() {
    let key = [0x42u8; 32];
    let id = compute_anonymous_id(&key, 1000);

    // The anonymous_id field is [u8; 32] — enforced at compile time,
    // but verify the runtime value is non-trivial (not all zeros)
    assert_eq!(id.len(), 32);
    assert_ne!(id, [0u8; 32], "Anonymous ID should not be all zeros");

    // Also verify via AnonymousSender struct
    let sender = AnonymousSender::compute(&key, 1000);
    assert_eq!(sender.anonymous_id.len(), 32);
    assert_eq!(sender.anonymous_id, id);
}

// @scenario: anonymous_sender.feature:Anonymous ID is 32 bytes
#[test]
fn test_anonymous_id_derived_via_hkdf_is_full_entropy() {
    // Verify the output looks like a proper HKDF derivation: high entropy,
    // no obvious patterns. Check byte distribution across multiple keys.
    let mut unique_bytes = std::collections::HashSet::new();
    for i in 0u8..50 {
        let key = [i; 32];
        let id = compute_anonymous_id(&key, 1000);
        unique_bytes.extend(id.iter().copied());
    }
    // 50 different 32-byte IDs should use a large portion of the byte space
    assert!(
        unique_bytes.len() > 200,
        "HKDF output should use diverse byte values (got {} unique out of 256)",
        unique_bytes.len()
    );
}

// ============================================================
// Adversarial Tests (CC-14)
// Security boundaries need parameterized tests with adversarial payloads:
// empty, max-length, null bytes, tampered, injection.
// ============================================================

/// CC-14: All-zeros shared key must still produce a valid, non-zero anonymous ID.
/// An attacker cannot force a "null" anonymous ID by manipulating the shared key.
#[test]
fn test_adversarial_zero_key_produces_valid_id() {
    let zero_key = [0u8; 32];
    let id = compute_anonymous_id(&zero_key, 1000);
    assert_ne!(id, [0u8; 32], "Zero key must not produce zero ID");
    assert_ne!(id, zero_key, "ID must not equal the input key");
}

/// CC-14: All-ones shared key must produce a valid, distinct anonymous ID.
#[test]
fn test_adversarial_ones_key_produces_valid_id() {
    let ones_key = [0xFFu8; 32];
    let id = compute_anonymous_id(&ones_key, 1000);
    assert_ne!(id, [0u8; 32]);
    assert_ne!(
        id, [0xFFu8; 32],
        "ID must not be trivially derived from key"
    );
}

/// CC-14: Epoch 0 and epoch u64::MAX are valid edge cases.
#[test]
fn test_adversarial_extreme_epochs() {
    let key = [0x42u8; 32];

    let id_zero = compute_anonymous_id(&key, 0);
    let id_max = compute_anonymous_id(&key, u64::MAX);

    assert_ne!(id_zero, [0u8; 32]);
    assert_ne!(id_max, [0u8; 32]);
    assert_ne!(
        id_zero, id_max,
        "Epoch 0 and MAX must produce different IDs"
    );
}

/// CC-14: Cross-epoch correlation attack — an adversary observing IDs across
/// consecutive epochs should not be able to link them to the same sender.
/// We verify statistical independence: no shared prefix, suffix, or XOR pattern.
#[test]
fn test_adversarial_cross_epoch_correlation_attempt() {
    let key = [0x42u8; 32];
    let epoch_base = 10_000u64;

    let ids: Vec<[u8; 32]> = (0..20)
        .map(|i| compute_anonymous_id(&key, epoch_base + i))
        .collect();

    // Check: no two IDs share the same first 4 bytes (birthday bound is fine for 20 samples)
    let prefixes: std::collections::HashSet<[u8; 4]> =
        ids.iter().map(|id| [id[0], id[1], id[2], id[3]]).collect();
    assert_eq!(
        prefixes.len(),
        ids.len(),
        "All 4-byte prefixes should be unique across epochs"
    );

    // Check: XOR of consecutive IDs should look random (not constant or predictable)
    let xors: Vec<[u8; 32]> = ids
        .windows(2)
        .map(|pair| {
            let mut xor = [0u8; 32];
            for (i, byte) in xor.iter_mut().enumerate() {
                *byte = pair[0][i] ^ pair[1][i];
            }
            xor
        })
        .collect();

    // All XOR diffs must be unique (no repeating pattern)
    let unique_xors: std::collections::HashSet<[u8; 32]> = xors.iter().copied().collect();
    assert_eq!(
        unique_xors.len(),
        xors.len(),
        "XOR diffs between consecutive epoch IDs should all be unique"
    );
}

/// CC-14: Replay of stale sender IDs — IDs from 2+ epochs ago must fail resolution.
#[test]
fn test_adversarial_replay_stale_sender_ids() {
    let key = SymmetricKey::generate();
    let contact = make_contact_with_key("Victim", key.clone());
    let contacts = vec![contact];

    let original_epoch = 1000;
    let captured_id = compute_anonymous_id(contacts[0].shared_key().as_bytes(), original_epoch);

    // Replaying at epoch+1 (within tolerance) — resolves
    assert!(
        resolve_sender(&contacts, &captured_id, original_epoch + 1).is_some(),
        "Epoch+1 replay should resolve (within tolerance)"
    );

    // Replaying at epoch+2 (beyond tolerance) — must fail
    assert!(
        resolve_sender(&contacts, &captured_id, original_epoch + 2).is_none(),
        "Epoch+2 replay must fail"
    );

    // Replaying at epoch+100 (far future) — must fail
    assert!(
        resolve_sender(&contacts, &captured_id, original_epoch + 100).is_none(),
        "Far-future replay must fail"
    );

    // Replaying at epoch+1000 — must fail
    assert!(
        resolve_sender(&contacts, &captured_id, original_epoch + 1000).is_none(),
        "Very far-future replay must fail"
    );
}

/// CC-14: Random/malformed anonymous IDs should never resolve to any contact.
#[test]
fn test_adversarial_random_ids_never_resolve() {
    let key = SymmetricKey::generate();
    let contacts = vec![make_contact_with_key("Alice", key)];

    let adversarial_ids: Vec<[u8; 32]> = vec![
        [0u8; 32],    // all zeros
        [0xFFu8; 32], // all ones
        [0x80u8; 32], // high bit set
        {
            // null bytes alternating
            let mut id = [0u8; 32];
            for (i, byte) in id.iter_mut().enumerate() {
                *byte = if i % 2 == 0 { 0 } else { 0xFF };
            }
            id
        },
        {
            // sequential bytes
            let mut id = [0u8; 32];
            for (i, byte) in id.iter_mut().enumerate() {
                *byte = i as u8;
            }
            id
        },
    ];

    for (i, bad_id) in adversarial_ids.iter().enumerate() {
        let result = resolve_sender(&contacts, bad_id, 1000);
        assert!(
            result.is_none(),
            "Adversarial ID pattern {} should not resolve to any contact",
            i
        );
    }
}

/// CC-14: SenderIndex must also reject adversarial IDs.
#[test]
fn test_adversarial_sender_index_rejects_crafted_ids() {
    let key = SymmetricKey::generate();
    let contacts = vec![make_contact_with_key("Alice", key)];

    let index = SenderIndex::build(&contacts, 1000);

    // Try all-zeros, all-ones, and sequential pattern
    assert!(index.resolve(&[0u8; 32]).is_none());
    assert!(index.resolve(&[0xFFu8; 32]).is_none());
    let mut seq = [0u8; 32];
    for (i, byte) in seq.iter_mut().enumerate() {
        *byte = i as u8;
    }
    assert!(index.resolve(&seq).is_none());
}

/// CC-14: Two contacts with nearly-identical keys (1 bit different) must produce
/// completely different anonymous IDs — no partial collision.
#[test]
fn test_adversarial_near_collision_keys() {
    let key1 = [0x42u8; 32];
    let mut key2 = key1;
    key2[31] ^= 0x01; // Flip one bit

    let epoch = 1000;
    let id1 = compute_anonymous_id(&key1, epoch);
    let id2 = compute_anonymous_id(&key2, epoch);

    assert_ne!(id1, id2, "1-bit key difference must produce different IDs");

    // Check that the IDs differ in many bytes (avalanche effect)
    let differing_bytes = id1.iter().zip(id2.iter()).filter(|(a, b)| a != b).count();
    assert!(
        differing_bytes >= 16,
        "HKDF should show avalanche effect: {} of 32 bytes differ (expected >= 16)",
        differing_bytes
    );
}

#[test]
fn test_hkdf_context_ensures_domain_separation() {
    // Verify that the anonymous ID derivation uses a unique context
    // by checking that raw HKDF with the same key+epoch but different
    // context produces a different result.
    use vauchi_core::crypto::HKDF;

    let key = [0x42u8; 32];
    let epoch: u64 = 1000;
    let epoch_bytes = epoch.to_le_bytes();

    // Compute with the production context
    let anon_id = compute_anonymous_id(&key, epoch);

    // Compute with a different context (simulating cross-protocol confusion)
    let mut wrong_info = b"Wrong_Context_v1".to_vec();
    wrong_info.extend_from_slice(&epoch_bytes);
    let wrong_id = *HKDF::derive_key(None, &key, &wrong_info);

    assert_ne!(
        anon_id, wrong_id,
        "Different HKDF contexts must produce different IDs"
    );
}
