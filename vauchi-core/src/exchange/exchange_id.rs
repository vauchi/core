// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unique identifier for an exchange session.
//!
//! [`ExchangeId`] is a 32-byte random value serialized as a 64-character
//! lowercase hex string. It uniquely identifies a single contact-exchange
//! attempt and is safe to share publicly (no sensitive data).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// A 32-byte random identifier for an exchange session.
///
/// Serialized as a 64-character lowercase hex string in JSON.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExchangeId([u8; 32]);

impl ExchangeId {
    /// Generate a new random `ExchangeId` using the OS CSPRNG.
    pub fn generate() -> Self {
        use rand::RngCore;
        use rand::rngs::OsRng;
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Construct an `ExchangeId` from raw bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Return a reference to the underlying 32-byte array.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ExchangeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ExchangeId({})", self)
    }
}

// ── Custom serde: hex string ─────────────────────────────────────────────────

mod hex_bytes {
    use super::ExchangeId;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(id: &ExchangeId, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&id.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<ExchangeId, D::Error> {
        let s = String::deserialize(d)?;
        if s.len() != 64 {
            return Err(serde::de::Error::custom(format!(
                "ExchangeId hex must be 64 chars, got {}",
                s.len()
            )));
        }
        let decoded = hex::decode(&s)
            .map_err(|e| serde::de::Error::custom(format!("ExchangeId hex decode error: {}", e)))?;
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&decoded);
        Ok(ExchangeId(bytes))
    }
}

impl Serialize for ExchangeId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        hex_bytes::serialize(self, s)
    }
}

impl<'de> Deserialize<'de> for ExchangeId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        hex_bytes::deserialize(d)
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: tests access ExchangeId internals (hex_bytes module) not exposed publicly
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_produces_unique_ids() {
        let a = ExchangeId::generate();
        let b = ExchangeId::generate();
        assert_ne!(a, b, "two generated IDs must differ");
    }

    #[test]
    fn from_bytes_roundtrip() {
        let bytes = [0xABu8; 32];
        let id = ExchangeId::from_bytes(bytes);
        assert_eq!(id.as_bytes(), &bytes);
    }

    #[test]
    fn serde_roundtrip() {
        let id = ExchangeId::generate();
        let json = serde_json::to_string(&id).expect("serialize");
        let decoded: ExchangeId = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(id, decoded);
    }

    #[test]
    fn display_is_hex() {
        let id = ExchangeId::from_bytes([0xCAu8; 32]);
        let s = id.to_string();
        assert_eq!(s.len(), 64, "display must be 64 hex chars");
        assert!(
            s.chars().all(|c| c.is_ascii_hexdigit()),
            "display must be lowercase hex"
        );
        assert_eq!(s, "ca".repeat(32));
    }

    // CC-11: failure paths — deserializer must reject malformed inputs
    #[test]
    fn deserialize_rejects_wrong_length() {
        // 32 hex chars (only 16 bytes decoded) — need 64 hex chars (32 bytes)
        let short = format!("\"{}\"", "ab".repeat(16));
        let result = serde_json::from_str::<ExchangeId>(&short);
        assert!(result.is_err(), "should reject 32-char hex (need 64)");
    }

    #[test]
    fn deserialize_rejects_invalid_hex() {
        // 64 chars but not valid hex
        let bad = format!("\"{}\"", "zz".repeat(32));
        let result = serde_json::from_str::<ExchangeId>(&bad);
        assert!(result.is_err(), "should reject non-hex characters");
    }

    #[test]
    fn deserialize_rejects_empty_string() {
        let result = serde_json::from_str::<ExchangeId>("\"\"");
        assert!(result.is_err(), "should reject empty string");
    }
}
