// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The parse-boundary bounds of the V2 wire: every string and list field
//! is read through a visitor that refuses more bytes or items than the
//! relay accepts, so an oversized body fails before any handler sees it.
//! Split out of `v2.rs` for size (VRS04); the serde attributes on the
//! types name these functions by path.

use super::{
    MAX_CIPHERTEXT_B64_LEN, MAX_GUARDIAN_ENTRIES, MAX_GUARDIAN_ENTRY_B64_LEN, MAX_GUARDIANS_LEN,
    MAX_ID_HEX_LEN, MAX_MAILBOX_TOKENS, MAX_ORIGIN_HINT_B64_LEN, MAX_PROOF_DATA_B64_LEN,
    MAX_RECOVERY_PROOFS_LEN, MAX_RECOVERY_QUERY_HASHES, MAX_SIGNATURE_HEX_LEN, MAX_STRING_LEN,
    V2GuardianEntry, V2RecoveryProof,
};
use serde::Deserialize;
use serde::de::{self, Deserializer, Error as DeError, Visitor};
use std::fmt;
use std::marker::PhantomData;

// =========================================================================
// Bounded deserialization helpers
// =========================================================================

pub(super) fn deserialize_bounded_string<'de, D>(
    deserializer: D,
    max_len: usize,
) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundedStringVisitor(usize);

    impl<'de> Visitor<'de> for BoundedStringVisitor {
        type Value = String;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a string of at most {} bytes", self.0)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() > self.0 {
                return Err(E::custom(format!(
                    "string too long: {} bytes (max {})",
                    v.len(),
                    self.0
                )));
            }
            Ok(v.to_owned())
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            if v.len() > self.0 {
                return Err(E::custom(format!(
                    "string too long: {} bytes (max {})",
                    v.len(),
                    self.0
                )));
            }
            Ok(v)
        }
    }

    deserializer.deserialize_str(BoundedStringVisitor(max_len))
}

pub(super) fn deserialize_bounded_vec_string<'de, D>(
    deserializer: D,
    max_items: usize,
    max_len: usize,
) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct BoundedVecStringVisitor {
        max_items: usize,
        max_len: usize,
    }

    impl<'de> Visitor<'de> for BoundedVecStringVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(
                formatter,
                "a list of at most {} strings, each at most {} bytes",
                self.max_items, self.max_len
            )
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(self.max_items));
            while let Some(value) = seq.next_element::<String>()? {
                if value.len() > self.max_len {
                    return Err(A::Error::custom(format!(
                        "string too long: {} bytes (max {})",
                        value.len(),
                        self.max_len
                    )));
                }
                if vec.len() >= self.max_items {
                    return Err(A::Error::custom(format!(
                        "too many items (max {})",
                        self.max_items
                    )));
                }
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_seq(BoundedVecStringVisitor { max_items, max_len })
}

pub(super) fn deserialize_bounded_vec<'de, D, T>(
    deserializer: D,
    max_items: usize,
) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct BoundedVecVisitor<T> {
        max_items: usize,
        marker: PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for BoundedVecVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Vec<T>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "a list of at most {} items", self.max_items)
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(self.max_items));
            while let Some(value) = seq.next_element::<T>()? {
                if vec.len() >= self.max_items {
                    return Err(A::Error::custom(format!(
                        "too many items (max {})",
                        self.max_items
                    )));
                }
                vec.push(value);
            }
            Ok(vec)
        }
    }

    deserializer.deserialize_seq(BoundedVecVisitor {
        max_items,
        marker: PhantomData,
    })
}

pub(super) fn deserialize_option_bounded_vec<'de, D, T>(
    deserializer: D,
    max_items: usize,
) -> Result<Option<Vec<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct OptionBoundedVecVisitor<T> {
        max_items: usize,
        marker: PhantomData<T>,
    }

    impl<'de, T> Visitor<'de> for OptionBoundedVecVisitor<T>
    where
        T: Deserialize<'de>,
    {
        type Value = Option<Vec<T>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(
                formatter,
                "null or a list of at most {} items",
                self.max_items
            )
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_bounded_vec(deserializer, self.max_items).map(Some)
        }
    }

    deserializer.deserialize_option(OptionBoundedVecVisitor {
        max_items,
        marker: PhantomData,
    })
}

// Per-field wrappers (required by serde's `deserialize_with` attribute).

pub(super) fn deserialize_recipient_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_blob_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_ciphertext<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

pub(super) fn deserialize_mailbox_tokens<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec_string(deserializer, MAX_MAILBOX_TOKENS, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_hex_hash<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_public_key<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_purge_token<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_signature<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_SIGNATURE_HEX_LEN)
}

pub(super) fn deserialize_exchange_payload<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

pub(super) fn deserialize_exchange_code<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_STRING_LEN)
}

pub(super) fn deserialize_exchange_response<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

pub(super) fn deserialize_proof_data<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_PROOF_DATA_B64_LEN)
}

pub(super) fn deserialize_key_hashes<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec_string(deserializer, MAX_RECOVERY_QUERY_HASHES, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_guardian_entries<'de, D>(
    deserializer: D,
) -> Result<Vec<V2GuardianEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_vec(deserializer, MAX_GUARDIAN_ENTRIES)
}

pub(super) fn deserialize_guardian_entry_data<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_GUARDIAN_ENTRY_B64_LEN)
}

pub(super) fn deserialize_designator_pk<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_status<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_bounded_string(deserializer, MAX_STRING_LEN)
}

pub(super) fn deserialize_optional_bounded_string<'de, D>(
    deserializer: D,
    max_len: usize,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionalBoundedStringVisitor(usize);

    impl<'de> Visitor<'de> for OptionalBoundedStringVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            write!(formatter, "null or a string of at most {} bytes", self.0)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_bounded_string(deserializer, self.0).map(Some)
        }
    }

    deserializer.deserialize_option(OptionalBoundedStringVisitor(max_len))
}

pub(super) fn deserialize_optional_error<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_STRING_LEN)
}

pub(super) fn deserialize_optional_blob_id<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_optional_code<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_STRING_LEN)
}

pub(super) fn deserialize_optional_payload<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

pub(super) fn deserialize_optional_response<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_CIPHERTEXT_B64_LEN)
}

pub(super) fn deserialize_optional_mailbox_token<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_optional_public_key<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ID_HEX_LEN)
}

pub(super) fn deserialize_optional_signature<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_SIGNATURE_HEX_LEN)
}

pub(super) fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) fn deserialize_optional_origin_hint<'de, D>(
    deserializer: D,
) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_optional_bounded_string(deserializer, MAX_ORIGIN_HINT_B64_LEN)
}

pub(super) fn deserialize_proofs<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<V2RecoveryProof>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_option_bounded_vec(deserializer, MAX_RECOVERY_PROOFS_LEN)
}

pub(super) fn deserialize_guardians<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<V2GuardianEntry>>, D::Error>
where
    D: Deserializer<'de>,
{
    deserialize_option_bounded_vec(deserializer, MAX_GUARDIANS_LEN)
}
