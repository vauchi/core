// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Strongly-typed identifier newtypes for swap-argument safety.
//!
//! Wraps raw `[u8; 32]` values that share a shape but mean different
//! things (e.g. `old_pk`, `new_pk`, `voucher_pk` on the recovery
//! structs; `identity_public_key` vs `dh_public` on the network
//! protocol). Accessors that previously returned `&[u8; 32]` now
//! return a kind-specific newtype, so cross-kind swaps at call
//! boundaries become compile errors as more kinds are introduced
//! (Phase 1B / 2 / 3 of the
//! `2026-05-21-wire-identifier-newtypes` problem record).
//!
//! Five distinct kinds today:
//! - [`IdentityKey`]: Ed25519 identity public key.
//! - [`DhPublicKey`]: X25519 Diffie–Hellman / X3DH public key.
//! - [`MailboxToken`]: 32-byte HMAC-derived mailbox routing token.
//! - [`ContactId`]: hex-fingerprint / UUID wire identifier on
//!   `sender_id` / `recipient_id` fields.
//! - [`MessageId`]: wire message identifier (UUID-v4 string).
//!
//! The three byte-kinds are nominally distinct so a swap at a call
//! site (Ed25519 ↔ X25519, shared_key/master_seed ↔ token) fails to
//! compile. `ContactId` and `MessageId` tag the two string-shaped
//! wire identifiers so a sender ↔ recipient or envelope-id ↔ ack-id
//! swap at a construction site fails to compile.
//!
//! Equality with bare `[u8; 32]` is **forward-only**: `key ==
//! expected_bytes` compiles (for test ergonomics) but
//! `expected_bytes == key` does not. The asymmetry rules out the
//! silently-symmetric weakness `assert_eq!(ed25519_bytes, dh_key)`
//! flagged in the Phase 1B audit while keeping the existing
//! `assert_eq!(key, &expected_bytes)` assertion pattern (used at
//! ~30 sites) working. [`ContactId`] mirrors the same asymmetry
//! against `&str`.
//!
//! Wire-shape stability is preserved two ways:
//! - The newtype's default serde is `#[serde(transparent)]`, so the
//!   on-disk JSON shape of `RecoveryProgress` (Phase 1A) does not
//!   change across the newtype migration. `ContactId` serializes as
//!   a raw JSON string for the same reason.
//! - Wire-active fields opt into base64 via per-field
//!   `#[serde(with = "wire_identity_key_base64")]` (or
//!   `wire_dh_public_key_base64`), which mirrors the existing
//!   `bytes_array_32` shape used by `network::message`,
//!   `sync::device_sync`, and `exchange::encrypted_message`.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte Ed25519 public key, opaque at the type level.
///
/// Tags identity-signing public keys (`old_pk` / `new_pk` /
/// `voucher_pk` on the recovery structs in Phase 1A; the wire-active
/// `identity_public_key`, `public_key`, and `identity_key` fields
/// across `network::message`, `sync::device_sync`, and
/// `exchange::encrypted_message` in Phase 1B). Distinct from
/// [`DhPublicKey`] so an Ed25519 ↔ X25519 mix-up at a call site
/// fails to compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, zeroize::Zeroize)]
#[serde(transparent)]
pub struct IdentityKey([u8; 32]);

impl IdentityKey {
    /// Wraps raw bytes into an `IdentityKey`.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Consumes the `IdentityKey`, returning the underlying bytes.
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for IdentityKey {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8; 32]> for IdentityKey {
    fn from(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }
}

impl AsRef<[u8]> for IdentityKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; 32]> for IdentityKey {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Forward-only cross-equality with raw bytes — see module docs.
/// Keeps `assert_eq!(thing.identity_key_accessor(), &expected_bytes)`
/// working without forcing every assertion site to wrap the literal
/// in `IdentityKey::from(...)`. The inverse
/// `impl PartialEq<IdentityKey> for [u8; 32]` is deliberately absent
/// so `assert_eq!(arbitrary_bytes, an_identity_key)` does NOT
/// silently compile.
impl PartialEq<[u8; 32]> for IdentityKey {
    fn eq(&self, other: &[u8; 32]) -> bool {
        &self.0 == other
    }
}

impl fmt::Display for IdentityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// A 32-byte X25519 Diffie–Hellman public key, opaque at the type
/// level.
///
/// Tags X3DH / Double-Ratchet public keys (`dh_public` on
/// `RatchetHeader`; `sender_exchange_key`, `ephemeral_public_key`,
/// and the inner `exchange_key` on the encrypted exchange message).
/// Distinct from [`IdentityKey`] so an Ed25519 ↔ X25519 mix-up at
/// a call site fails to compile. Same byte layout as `IdentityKey`
/// (32 bytes) but a different cryptographic primitive — passing an
/// Ed25519 identity key where an X25519 DH key is expected is a
/// real bug even though both serialize identically on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, zeroize::Zeroize)]
#[serde(transparent)]
pub struct DhPublicKey([u8; 32]);

impl DhPublicKey {
    /// Wraps raw bytes into a `DhPublicKey`.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Consumes the `DhPublicKey`, returning the underlying bytes.
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for DhPublicKey {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8; 32]> for DhPublicKey {
    fn from(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }
}

impl AsRef<[u8]> for DhPublicKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; 32]> for DhPublicKey {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Forward-only cross-equality, mirroring [`IdentityKey`]'s impl.
/// The inverse is deliberately absent so
/// `assert_eq!(ed25519_bytes, a_dh_public_key)` does NOT silently
/// compile across crypto-primitive boundaries.
impl PartialEq<[u8; 32]> for DhPublicKey {
    fn eq(&self, other: &[u8; 32]) -> bool {
        &self.0 == other
    }
}

impl fmt::Display for DhPublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Per-field serde adapter that serializes [`IdentityKey`] as a
/// base64 string (matching the legacy `bytes_array_32` shape on
/// `network::message`, `sync::device_sync`, and
/// `exchange::encrypted_message`).
///
/// The newtype's own derived serde stays `#[serde(transparent)]`
/// (preserving the Phase 1A `RecoveryProgress` JSON-array shape).
/// Wire fields opt into base64 explicitly via
/// `#[serde(with = "vauchi_core::identifiers::wire_identity_key_base64")]`.
pub mod wire_identity_key_base64 {
    use super::IdentityKey;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes an `IdentityKey` as a base64 string.
    pub fn serialize<S>(key: &IdentityKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&BASE64.encode(key.as_bytes()))
    }

    /// Deserializes a base64 string into an `IdentityKey`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<IdentityKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = BASE64.decode(&s).map_err(serde::de::Error::custom)?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte IdentityKey"))?;
        Ok(IdentityKey::from_bytes(bytes))
    }
}

/// Per-field serde adapter that serializes [`DhPublicKey`] as a
/// base64 string. Same wire shape as
/// [`wire_identity_key_base64`]; the nominal-type distinction lives
/// only at the Rust level.
pub mod wire_dh_public_key_base64 {
    use super::DhPublicKey;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serializes a `DhPublicKey` as a base64 string.
    pub fn serialize<S>(key: &DhPublicKey, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&BASE64.encode(key.as_bytes()))
    }

    /// Deserializes a base64 string into a `DhPublicKey`.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<DhPublicKey, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = BASE64.decode(&s).map_err(serde::de::Error::custom)?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("invalid length for 32-byte DhPublicKey"))?;
        Ok(DhPublicKey::from_bytes(bytes))
    }
}

/// A wire-level contact identifier, opaque at the type level.
///
/// Tags `sender_id` / `recipient_id` fields on wire structs
/// (`EncryptedUpdate`, `IdentityRevoked`, `EmergencyAlert`, and the
/// `Simple*` variants). Distinct from a bare `String` so a
/// sender ↔ recipient swap at a construction site fails to compile:
/// `EncryptedUpdate { recipient_id: their_id, sender_id: my_id, ... }`
/// cannot silently flip its arguments when both ends are typed.
///
/// The underlying value is the hex-encoded fingerprint of a contact's
/// 32-byte signing public key (for exchanged contacts) or a UUID
/// (for imported contacts) — the wire shape is a single string
/// either way, and that shape is preserved via `#[serde(transparent)]`.
///
/// Equality with `&str` is **forward-only**: `id == "deadbeef…"`
/// compiles (for test ergonomics) but `"deadbeef…" == id` does not,
/// mirroring the asymmetry on [`IdentityKey`] and [`DhPublicKey`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ContactId(String);

impl ContactId {
    /// Wraps a `String` into a `ContactId`.
    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    /// Borrows the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `ContactId`, returning the underlying `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for ContactId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for ContactId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for ContactId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<[u8]> for ContactId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

/// Forward-only cross-equality with `&str` — see type docs.
/// Keeps `assert_eq!(thing.contact_id_accessor(), "deadbeef…")`
/// working without forcing every assertion site to wrap the literal
/// in `ContactId::from(...)`. The inverse
/// `impl PartialEq<ContactId> for str` is deliberately absent so
/// `assert_eq!("deadbeef…", a_contact_id)` does NOT silently compile.
impl PartialEq<str> for ContactId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for ContactId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for ContactId {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl fmt::Display for ContactId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A wire-level message identifier, opaque at the type level.
///
/// Tags the `message_id` field on `MessageEnvelope`, `Acknowledgment`,
/// and any other wire struct or in-memory map that keys off a relay
/// message ID. Replaces the legacy `pub type MessageId = String;`
/// alias so an accidental swap (envelope id ↔ ack target id, or
/// message id ↔ contact id at a `HashMap<String, _>` boundary)
/// fails to compile.
///
/// The underlying value is a UUID-v4 string today, generated at
/// envelope-creation time. `#[serde(transparent)]` keeps the wire
/// shape — a raw JSON string — byte-identical to the bare alias.
///
/// Equality with `&str` and `String` is **forward-only**, mirroring
/// [`ContactId`] and the byte-key newtypes.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MessageId(String);

impl MessageId {
    /// Wraps a `String` into a `MessageId`.
    pub fn from_string(s: String) -> Self {
        Self(s)
    }

    /// Borrows the underlying string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `MessageId`, returning the underlying `String`.
    pub fn into_string(self) -> String {
        self.0
    }
}

impl From<String> for MessageId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for MessageId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl AsRef<str> for MessageId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<[u8]> for MessageId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl PartialEq<str> for MessageId {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for MessageId {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for MessageId {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A 32-byte HMAC-derived mailbox routing token, opaque at the
/// type level.
///
/// Returned by [`crate::network::mailbox_token::compute_mailbox_token`]
/// (contact tokens, derived from a shared key + day epoch) and
/// [`crate::network::mailbox_token::compute_self_token`] (self
/// tokens, derived from the identity master seed + day epoch). The
/// 32-byte form lives only in memory; the wire form is always the
/// lowercase-hex string produced by
/// [`crate::network::mailbox_token::token_hex`] (carried inside
/// `RegisterMailbox.tokens: Vec<String>` or
/// `EncryptedUpdate.recipient_id` per ADR-029).
///
/// Distinct from [`IdentityKey`] and [`DhPublicKey`] so a
/// shared_key / master_seed ↔ token swap at a call site fails to
/// compile. Same byte layout as the other 32-byte newtypes — the
/// distinction is purely nominal.
///
/// Equality with raw `[u8; 32]` is **forward-only**, mirroring the
/// other byte-key newtypes' audit hardening.
#[derive(Clone, Debug, PartialEq, Eq, Hash, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct MailboxToken([u8; 32]);

impl MailboxToken {
    /// Wraps raw bytes into a `MailboxToken`.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Consumes the `MailboxToken`, returning the underlying bytes.
    /// The wrapper's `ZeroizeOnDrop` no longer runs on the returned
    /// array — callers that take the raw form are expected to manage
    /// zeroization themselves.
    pub fn into_bytes(mut self) -> [u8; 32] {
        let out = self.0;
        // Prevent the Drop impl from zeroizing the bytes we just
        // handed out. `take()` swaps in zeros locally, then we
        // forget the (now-zeroed) wrapper.
        self.0 = [0u8; 32];
        std::mem::forget(self);
        out
    }
}

impl From<[u8; 32]> for MailboxToken {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8; 32]> for MailboxToken {
    fn from(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }
}

impl AsRef<[u8]> for MailboxToken {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; 32]> for MailboxToken {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Forward-only cross-equality with raw bytes, mirroring
/// [`IdentityKey`] / [`DhPublicKey`].
impl PartialEq<[u8; 32]> for MailboxToken {
    fn eq(&self, other: &[u8; 32]) -> bool {
        &self.0 == other
    }
}

impl fmt::Display for MailboxToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
