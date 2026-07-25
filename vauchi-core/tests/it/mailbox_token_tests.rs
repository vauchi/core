// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::network::mailbox_token::{
    batch_register_tokens, batch_register_tokens_with_device_sync, compute_device_mailbox_token,
    compute_device_sync_token, compute_mailbox_token, compute_self_token, token_hex,
};

// Directional tokens are keyed to the recipient. These tests assert only
// determinism/uniqueness/shape (not routing), so a single fixed recipient
// pubkey keeps comparisons honest — the varied input is the shared key or day.
const TEST_PUBKEY: [u8; 32] = [0xAAu8; 32];

// @internal
#[test]
fn test_contact_mailbox_token_deterministic() {
    let shared_key = [0x42u8; 32];
    let day = 19804u64;
    let t1 = compute_mailbox_token(&shared_key, &TEST_PUBKEY, day);
    let t2 = compute_mailbox_token(&shared_key, &TEST_PUBKEY, day);
    assert_eq!(t1, t2);
    assert_eq!(t1.as_bytes().len(), 32);
}

// @internal
#[test]
fn test_contact_mailbox_token_rotates_daily() {
    let shared_key = [0x42u8; 32];
    let day1 = compute_mailbox_token(&shared_key, &TEST_PUBKEY, 19804);
    let day2 = compute_mailbox_token(&shared_key, &TEST_PUBKEY, 19805);
    assert_ne!(day1, day2);
}

// @scenario: multi_device_sync :: Each device has its own contact mailbox
// @internal
#[test]
fn test_device_mailbox_token_is_device_specific_daily_and_distinct_from_identity() {
    // F4 device-scoped contact mailbox (ADR-064 Amendment 2026-07-25): the
    // token folds the recipient device id into HKDF input so a sibling
    // cannot drain another sibling's copy — but stays daily-rotating with
    // no wire-visible correlator.
    let shared_key = [0x42u8; 32];
    let dev_a = [1u8; 32];
    let dev_b = [2u8; 32];
    let day = 19804u64;

    // Deterministic.
    assert_eq!(
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day),
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day),
    );
    // Device-specific.
    assert_ne!(
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day),
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_b, day),
    );
    // Daily-rotating.
    assert_ne!(
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day),
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day + 1),
    );
    // Never collides with the identity-scoped token (own domain input).
    assert_ne!(
        compute_device_mailbox_token(&shared_key, &TEST_PUBKEY, &dev_a, day).as_bytes(),
        compute_mailbox_token(&shared_key, &TEST_PUBKEY, day).as_bytes(),
    );
}

// @internal
#[test]
fn test_different_contacts_produce_different_tokens() {
    let key_a = [0x42u8; 32];
    let key_b = [0x43u8; 32];
    let day = 19804u64;
    let t_a = compute_mailbox_token(&key_a, &TEST_PUBKEY, day);
    let t_b = compute_mailbox_token(&key_b, &TEST_PUBKEY, day);
    assert_ne!(t_a, t_b);
}

// @internal
#[test]
fn test_self_token_deterministic_across_devices() {
    let master_seed = [0xAAu8; 32];
    let day = 19804u64;
    let t1 = compute_self_token(&master_seed, day);
    let t2 = compute_self_token(&master_seed, day);
    assert_eq!(t1, t2);
}

/// Device-sync delivery is one-recipient-per-fetch: two linked devices must
/// never poll the same opaque mailbox token or one can consume the other's
/// encrypted envelope from the relay.
// @scenario: sync_updates :: Linked-device updates reach every device
#[test]
fn test_device_sync_tokens_are_deterministic_and_recipient_specific() {
    let master_seed = [0xAAu8; 32];
    let day = 19804u64;
    let first_device = [0x01u8; 32];
    let second_device = [0x02u8; 32];

    assert_eq!(
        compute_device_sync_token(&master_seed, &first_device, day),
        compute_device_sync_token(&master_seed, &first_device, day),
        "a device must rederive its receive mailbox token"
    );
    assert_ne!(
        compute_device_sync_token(&master_seed, &first_device, day),
        compute_device_sync_token(&master_seed, &second_device, day),
        "different linked devices must not compete for one destructive mailbox"
    );
}

// @internal
#[test]
fn test_device_sync_registration_preserves_legacy_and_own_receive_tokens() {
    let master_seed = [0xBBu8; 32];
    let device_id = [0x11u8; 32];
    let day = 19804u64;
    let batches = batch_register_tokens_with_device_sync(
        &vauchi_core::rng::OsSecureRng::new(),
        &[],
        &TEST_PUBKEY,
        &master_seed,
        &device_id,
        day,
        0,
    );

    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 256);
    assert!(batches[0].contains(&token_hex(&compute_device_sync_token(
        &master_seed,
        &device_id,
        day,
    ))));
    assert!(batches[0].contains(&token_hex(&compute_device_sync_token(
        &master_seed,
        &device_id,
        day - 1,
    ))));
    assert!(batches[0].contains(&token_hex(&compute_self_token(&master_seed, day,))));
    assert!(batches[0].contains(&token_hex(&compute_self_token(&master_seed, day - 1,))));
}

// @scenario: multi_device_sync :: A per-device copy reaches only its own sibling
// @internal
#[test]
fn test_device_scoped_registration_isolates_siblings_from_each_others_copies() {
    // The F4 delivery-layer guarantee (ADR-064 Amendment 2026-07-25): with
    // the exchanging device gone, siblings A2/A3 must not drain each other's
    // per-device copies from a shared destructive mailbox. Each registers a
    // DISTINCT device-scoped receive token, so a copy Bob addresses to A2's
    // device is delivered only to A2.
    let shared_key = [0x77u8; 32];
    let alice_pubkey = TEST_PUBKEY;
    let a2_device = [0x22u8; 32];
    let a3_device = [0x33u8; 32];
    let alice_master = [0x99u8; 32];
    let day = 19804u64;

    let flatten =
        |batches: Vec<Vec<String>>| -> Vec<String> { batches.into_iter().flatten().collect() };
    let a2_tokens = flatten(batch_register_tokens_with_device_sync(
        &vauchi_core::rng::OsSecureRng::new(),
        &[shared_key],
        &alice_pubkey,
        &alice_master,
        &a2_device,
        day,
        0,
    ));
    let a3_tokens = flatten(batch_register_tokens_with_device_sync(
        &vauchi_core::rng::OsSecureRng::new(),
        &[shared_key],
        &alice_pubkey,
        &alice_master,
        &a3_device,
        day,
        0,
    ));

    // Bob's deposit token for the copy addressed to A2's device.
    let bob_copy_for_a2 = token_hex(&compute_device_mailbox_token(
        &shared_key,
        &alice_pubkey,
        &a2_device,
        day,
    ));

    assert!(
        a2_tokens.contains(&bob_copy_for_a2),
        "A2 must register the mailbox its own per-device copy lands in"
    );
    assert!(
        !a3_tokens.contains(&bob_copy_for_a2),
        "A3 must NOT poll A2's device mailbox — no sibling cross-consumption"
    );

    // The legacy identity mailbox is still shared (legacy [0;32]/genesis
    // sends), which is fine — only the per-device path needs isolation.
    let identity_token = token_hex(&compute_mailbox_token(&shared_key, &alice_pubkey, day));
    assert!(a2_tokens.contains(&identity_token));
    assert!(a3_tokens.contains(&identity_token));
}

// @internal
#[test]
fn test_self_token_differs_from_contact_token() {
    let key = [0x42u8; 32];
    let day = 19804u64;
    let contact = compute_mailbox_token(&key, &TEST_PUBKEY, day);
    let self_tok = compute_self_token(&key, day);
    assert_ne!(contact, self_tok);
}

// @internal
#[test]
fn test_token_hex_produces_64_char_hex() {
    let token = compute_mailbox_token(&[0x42u8; 32], &TEST_PUBKEY, 19804);
    let hex = token_hex(&token);
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

// @internal
#[test]
fn test_batch_tokens_padded_to_256() {
    let contacts: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
    let master_seed = [0xBBu8; 32];
    let batches = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    assert!(!batches.is_empty());
    for batch in &batches {
        assert_eq!(batch.len(), 256);
    }
}

// @internal
#[test]
fn test_batch_tokens_no_duplicates() {
    let contacts: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
    let master_seed = [0xBBu8; 32];
    let batches = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    assert_eq!(batches.len(), 1);
    let mut sorted = batches[0].clone();
    sorted.sort();
    sorted.dedup();
    assert!(sorted.len() >= 250);
}

// @internal
#[test]
fn test_batch_tokens_historical_catchup() {
    let contacts: Vec<[u8; 32]> = vec![[0x01; 32]];
    let master_seed = [0xBBu8; 32];
    let batches = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        3,
    );
    assert!(!batches.is_empty());
    for batch in &batches {
        assert_eq!(batch.len(), 256); // Each batch padded to 256
    }
}

// @internal
#[test]
fn test_batch_tokens_many_contacts_splits() {
    // 200 contacts × 2 tokens/day (+ previous-day skew) + 2 self tokens = 402 real tokens
    // This exceeds 256, so we expect at least 2 batches
    let contacts: Vec<[u8; 32]> = (0..200u8)
        .map(|i| {
            let mut k = [0u8; 32];
            k[0] = i;
            k[1] = i.wrapping_add(1);
            k
        })
        .collect();
    let master_seed = [0xCCu8; 32];
    let batches = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    assert!(
        batches.len() >= 2,
        "expected multiple batches for 200 contacts, got {}",
        batches.len()
    );
    for batch in &batches {
        assert_eq!(batch.len(), 256);
    }
}

// @internal
#[test]
fn test_batch_tokens_shuffled() {
    // Two calls with identical inputs should produce different orderings.
    // Padding tokens are random so sorted sets won't be equal; instead we
    // verify that the first-position token differs between calls, which is
    // overwhelmingly likely (probability of collision ≈ 1/2^256 per token).
    let contacts: Vec<[u8; 32]> = (0..10).map(|i| [i as u8; 32]).collect();
    let master_seed = [0xDDu8; 32];
    let batches_a = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    let batches_b = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &contacts,
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    assert_eq!(batches_a.len(), 1);
    assert_eq!(batches_b.len(), 1);
    assert_eq!(batches_a[0].len(), 256);
    assert_eq!(batches_b[0].len(), 256);
    // Shuffled batches virtually never share the same ordering
    assert_ne!(
        batches_a[0], batches_b[0],
        "unsorted batches should differ due to shuffle + random padding"
    );
}

// @internal
#[test]
fn test_batch_tokens_no_contacts_returns_one_batch() {
    let master_seed = [0xEEu8; 32];
    let batches = batch_register_tokens(
        &vauchi_core::rng::OsSecureRng::new(),
        &[],
        &TEST_PUBKEY,
        &master_seed,
        19804,
        0,
    );
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].len(), 256);
}

proptest! {
// @internal
    #[test]
    fn prop_different_contacts_produce_unlinkable_tokens(
        key_a in prop::array::uniform32(any::<u8>()),
        key_b in prop::array::uniform32(any::<u8>()),
        day in 0u64..100000,
    ) {
        prop_assume!(key_a != key_b);
        let t_a = compute_mailbox_token(&key_a, &TEST_PUBKEY, day);
        let t_b = compute_mailbox_token(&key_b, &TEST_PUBKEY, day);
        prop_assert_ne!(t_a, t_b);
    }
}
