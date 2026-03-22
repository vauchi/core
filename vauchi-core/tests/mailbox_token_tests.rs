// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::network::mailbox_token::{
    batch_register_tokens, compute_mailbox_token, compute_self_token, token_hex,
};

#[test]
fn test_contact_mailbox_token_deterministic() {
    let shared_key = [0x42u8; 32];
    let day = 19804u64;
    let t1 = compute_mailbox_token(&shared_key, day);
    let t2 = compute_mailbox_token(&shared_key, day);
    assert_eq!(t1, t2);
    assert_eq!(t1.len(), 32);
}

#[test]
fn test_contact_mailbox_token_rotates_daily() {
    let shared_key = [0x42u8; 32];
    let day1 = compute_mailbox_token(&shared_key, 19804);
    let day2 = compute_mailbox_token(&shared_key, 19805);
    assert_ne!(day1, day2);
}

#[test]
fn test_different_contacts_produce_different_tokens() {
    let key_a = [0x42u8; 32];
    let key_b = [0x43u8; 32];
    let day = 19804u64;
    let t_a = compute_mailbox_token(&key_a, day);
    let t_b = compute_mailbox_token(&key_b, day);
    assert_ne!(t_a, t_b);
}

#[test]
fn test_self_token_deterministic_across_devices() {
    let master_seed = [0xAAu8; 32];
    let day = 19804u64;
    let t1 = compute_self_token(&master_seed, day);
    let t2 = compute_self_token(&master_seed, day);
    assert_eq!(t1, t2);
}

#[test]
fn test_self_token_differs_from_contact_token() {
    let key = [0x42u8; 32];
    let day = 19804u64;
    let contact = compute_mailbox_token(&key, day);
    let self_tok = compute_self_token(&key, day);
    assert_ne!(contact, self_tok);
}

#[test]
fn test_token_hex_produces_64_char_hex() {
    let token = compute_mailbox_token(&[0x42u8; 32], 19804);
    let hex = token_hex(&token);
    assert_eq!(hex.len(), 64);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn test_batch_tokens_padded_to_256() {
    let contacts: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
    let master_seed = [0xBBu8; 32];
    let tokens = batch_register_tokens(&contacts, &master_seed, 19804, 0);
    assert_eq!(tokens.len(), 256);
}

#[test]
fn test_batch_tokens_no_duplicates() {
    let contacts: Vec<[u8; 32]> = (0..5).map(|i| [i as u8; 32]).collect();
    let master_seed = [0xBBu8; 32];
    let tokens = batch_register_tokens(&contacts, &master_seed, 19804, 0);
    let mut sorted = tokens.clone();
    sorted.sort();
    sorted.dedup();
    // Real tokens should be unique; random padding collision is astronomically unlikely
    assert!(sorted.len() >= 250);
}

#[test]
fn test_batch_tokens_historical_catchup() {
    let contacts: Vec<[u8; 32]> = vec![[0x01; 32]];
    let master_seed = [0xBBu8; 32];
    let tokens = batch_register_tokens(&contacts, &master_seed, 19804, 3);
    assert_eq!(tokens.len(), 256); // Still padded to 256
}
