// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proptest Strategies
//!
//! Reusable proptest strategies for property-based testing.
//! Import these in property test files to avoid duplication.

use proptest::prelude::*;

// ============================================================
// String Strategies
// ============================================================

/// Strategy for generating valid display names (non-empty, reasonable length).
pub fn display_name_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z][a-zA-Z0-9 ]{0,49}"
        .prop_map(|s| s.trim().to_string())
        .prop_filter("non-empty", |s| !s.is_empty())
}

/// Strategy for generating field labels (lowercase, underscore-separated).
pub fn field_label_strategy() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_]{0,19}"
}

/// Strategy for generating field values (arbitrary printable content).
pub fn field_value_strategy() -> impl Strategy<Value = String> {
    ".{1,100}"
}

/// Strategy for generating email addresses.
pub fn email_strategy() -> impl Strategy<Value = String> {
    ("[a-z]{3,10}", "[a-z]{2,8}", "[a-z]{2,4}")
        .prop_map(|(user, domain, tld)| format!("{}@{}.{}", user, domain, tld))
}

/// Strategy for generating phone numbers.
pub fn phone_strategy() -> impl Strategy<Value = String> {
    "[0-9]{10,15}".prop_map(|n| format!("+{}", n))
}

/// Strategy for generating URLs.
pub fn url_strategy() -> impl Strategy<Value = String> {
    ("[a-z]{3,10}", "[a-z]{2,4}")
        .prop_map(|(domain, tld)| format!("https://{}.{}", domain, tld))
}

/// Strategy for generating hex-encoded contact IDs (64 chars).
pub fn contact_id_strategy() -> impl Strategy<Value = String> {
    "[a-f0-9]{64}"
}

/// Strategy for generating device names.
pub fn device_name_strategy() -> impl Strategy<Value = String> {
    "(Phone|Tablet|Laptop|Desktop|Watch) [A-Z][a-z]{2,8}"
}

// ============================================================
// Byte Array Strategies
// ============================================================

/// Strategy for generating 32-byte arrays (keys, IDs).
pub fn bytes32_strategy() -> impl Strategy<Value = [u8; 32]> {
    prop::array::uniform32(any::<u8>())
}

/// Strategy for generating 64-byte arrays (signatures).
pub fn bytes64_strategy() -> impl Strategy<Value = [u8; 64]> {
    prop::array::uniform64(any::<u8>())
}

/// Strategy for generating variable-length byte vectors.
pub fn byte_vec_strategy(min: usize, max: usize) -> impl Strategy<Value = Vec<u8>> {
    prop::collection::vec(any::<u8>(), min..max)
}

// ============================================================
// Numeric Strategies
// ============================================================

/// Strategy for generating timestamps (reasonable Unix epoch range).
pub fn timestamp_strategy() -> impl Strategy<Value = u64> {
    1000000000u64..2000000000u64
}

/// Strategy for generating device indices.
pub fn device_index_strategy() -> impl Strategy<Value = u32> {
    0u32..100u32
}

/// Strategy for generating version numbers.
pub fn version_strategy() -> impl Strategy<Value = u64> {
    1u64..1000u64
}

/// Strategy for generating small counts (for loop iterations).
pub fn small_count_strategy() -> impl Strategy<Value = usize> {
    1usize..20usize
}

/// Strategy for generating medium counts.
pub fn medium_count_strategy() -> impl Strategy<Value = usize> {
    10usize..100usize
}

/// Strategy for generating large counts (for stress testing).
pub fn large_count_strategy() -> impl Strategy<Value = usize> {
    100usize..1000usize
}

// ============================================================
// Field Type Strategies
// ============================================================

/// Strategy for generating random FieldType values.
pub fn field_type_strategy() -> impl Strategy<Value = vauchi_core::FieldType> {
    prop_oneof![
        Just(vauchi_core::FieldType::Email),
        Just(vauchi_core::FieldType::Phone),
        Just(vauchi_core::FieldType::Website),
        Just(vauchi_core::FieldType::Address),
        Just(vauchi_core::FieldType::Social),
        Just(vauchi_core::FieldType::Custom),
    ]
}

// ============================================================
// Composite Strategies
// ============================================================

/// Strategy for generating (label, value) pairs.
pub fn field_data_strategy() -> impl Strategy<Value = (String, String)> {
    (field_label_strategy(), field_value_strategy())
}

/// Strategy for generating multiple field data pairs.
pub fn multi_field_strategy(count: usize) -> impl Strategy<Value = Vec<(String, String)>> {
    prop::collection::vec(field_data_strategy(), 1..=count)
}

/// Strategy for generating password-like strings (meets complexity).
pub fn password_strategy() -> impl Strategy<Value = String> {
    "[A-Z][a-z]{4,8}[0-9]{2,4}[!@#$%]"
}

/// Strategy for generating weak passwords (for rejection testing).
pub fn weak_password_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("password".to_string()),
        Just("12345678".to_string()),
        Just("qwerty".to_string()),
        "[a-z]{4,6}".prop_map(|s| s.to_string()),
    ]
}
