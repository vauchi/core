// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the G5 clipboard + storage-key policy helpers.

use vauchi_platform::{
    mobile_clipboard_policy, mobile_generate_storage_key, mobile_storage_key_byte_length,
};

// @internal
#[test]
fn default_clipboard_policy_is_thirty_seconds() {
    let policy = mobile_clipboard_policy();
    assert_eq!(policy.auto_clear_seconds, 30);
}

// @internal
#[test]
fn clipboard_policy_is_a_plain_record() {
    let a = mobile_clipboard_policy();
    let b = mobile_clipboard_policy();
    assert_eq!(
        a.auto_clear_seconds, b.auto_clear_seconds,
        "policy must be deterministic across calls"
    );
}

// @internal
#[test]
fn storage_key_byte_length_matches_symmetric_key() {
    assert_eq!(mobile_storage_key_byte_length(), 32);
}

// @internal
#[test]
fn generated_storage_key_is_expected_length() {
    let key = mobile_generate_storage_key();
    assert_eq!(key.len(), mobile_storage_key_byte_length() as usize);
}

// @internal
#[test]
fn generated_storage_keys_are_unique() {
    let a = mobile_generate_storage_key();
    let b = mobile_generate_storage_key();
    assert_ne!(a, b, "CSPRNG must not return identical keys in a row");
}

// @internal
#[test]
fn generated_storage_key_is_not_all_zeros() {
    let key = mobile_generate_storage_key();
    assert!(
        key.iter().any(|&b| b != 0),
        "storage key must have at least one non-zero byte"
    );
}
