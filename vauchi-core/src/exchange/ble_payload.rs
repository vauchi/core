// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Card Payload
//!
//! Full contact card payload for BLE encrypted exchange.
//! Unlike [`NfcCardPayload`] which carries only keys + name, this payload
//! includes contact fields and an optional avatar for complete card transfer.
//! Serialized with `postcard`, then encrypted before BLE transmission.
//! The CRC16 field is a sanity check — integrity is provided by AEAD encryption.

use serde::{Deserialize, Serialize};

use crate::contact_card::{ContactCard, ContactField, FieldType};

/// Full contact card payload exchanged during BLE proximity exchange.
///
/// Serialized with `postcard`, then encrypted with XChaCha20-Poly1305.
/// The CRC16 field is computed over all data fields (identity_key, display_name,
/// exchange_key, fields, avatar) and verified after decryption as a sanity check.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BleCardPayload {
    pub identity_key: [u8; 32],
    pub display_name: String,
    pub exchange_key: [u8; 32],
    pub fields: Vec<(String, String)>,
    pub avatar: Option<Vec<u8>>,
    pub crc16: u16,
}

impl BleCardPayload {
    /// Creates a new payload, computing CRC16 automatically.
    pub fn new(
        identity_key: [u8; 32],
        display_name: String,
        exchange_key: [u8; 32],
        fields: Vec<(String, String)>,
        avatar: Option<Vec<u8>>,
    ) -> Self {
        let crc = Self::compute_crc16(
            &identity_key,
            &display_name,
            &exchange_key,
            &fields,
            avatar.as_deref(),
        );
        Self {
            identity_key,
            display_name,
            exchange_key,
            fields,
            avatar,
            crc16: crc,
        }
    }

    /// Verifies the CRC16 matches the payload data.
    pub fn verify_crc16(&self) -> bool {
        let expected = Self::compute_crc16(
            &self.identity_key,
            &self.display_name,
            &self.exchange_key,
            &self.fields,
            self.avatar.as_deref(),
        );
        expected == self.crc16
    }

    /// Serializes to bytes using postcard.
    pub fn to_bytes(&self) -> Result<Vec<u8>, postcard::Error> {
        postcard::to_allocvec(self)
    }

    /// Deserializes from bytes using postcard.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, postcard::Error> {
        postcard::from_bytes(bytes)
    }

    /// Reconstruct a [`ContactCard`] from this received payload, for
    /// persisting the peer as an exchanged contact. The BLE wire format
    /// carries only `(label, value)` field tuples — the original
    /// [`FieldType`] is not transported — so every reconstructed field
    /// defaults to [`FieldType::Custom`]. A well-behaved peer sends a
    /// WebP ≤ 32 KB avatar (ADR-042 enforces this on the sender's own
    /// card); an avatar that fails that constraint is silently skipped
    /// (best-effort — `vauchi-core` has no logger, and a missing avatar
    /// must not fail the whole exchange).
    pub fn to_contact_card(&self, now: u64) -> ContactCard {
        let mut card = ContactCard::new(&self.display_name);
        for (label, value) in &self.fields {
            // `add_field` only errors on the per-card field cap; a peer
            // that sent more is truncated rather than failing the whole
            // exchange.
            if card
                .add_field(ContactField::new(FieldType::Custom, label, value, now))
                .is_err()
            {
                break;
            }
        }
        if let Some(avatar) = &self.avatar {
            // Best-effort per the doc above; an invalid avatar is skipped.
            let _ = card.set_avatar(avatar.clone());
        }
        card
    }

    /// CRC16/CCITT-FALSE over concatenated fields.
    fn compute_crc16(
        identity_key: &[u8; 32],
        display_name: &str,
        exchange_key: &[u8; 32],
        fields: &[(String, String)],
        avatar: Option<&[u8]>,
    ) -> u16 {
        let mut crc: u16 = 0xFFFF;

        let feed = |crc: &mut u16, byte: u8| {
            *crc ^= (byte as u16) << 8;
            for _ in 0..8 {
                if *crc & 0x8000 != 0 {
                    *crc = (*crc << 1) ^ 0x1021;
                } else {
                    *crc <<= 1;
                }
            }
        };

        for &b in identity_key {
            feed(&mut crc, b);
        }
        for b in display_name.as_bytes() {
            feed(&mut crc, *b);
        }
        for &b in exchange_key {
            feed(&mut crc, b);
        }
        for (key, value) in fields {
            for b in key.as_bytes() {
                feed(&mut crc, *b);
            }
            for b in value.as_bytes() {
                feed(&mut crc, *b);
            }
        }
        if let Some(avatar_bytes) = avatar {
            for &b in avatar_bytes {
                feed(&mut crc, b);
            }
        }

        crc
    }
}

// INLINE_TEST_REQUIRED: exercises the wire-payload → ContactCard
// reconstruction (field-type defaulting) without a storage round-trip.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn to_contact_card_carries_name_and_fields_defaulting_type_to_custom() {
        let payload = BleCardPayload::new(
            [7u8; 32],
            "Dana".into(),
            [8u8; 32],
            vec![
                ("Email".into(), "dana@example.com".into()),
                ("Phone".into(), "555".into()),
            ],
            None,
        );
        let card = payload.to_contact_card(1_000);

        assert_eq!(card.display_name(), "Dana");
        let fields: Vec<(&str, &str)> = card
            .fields()
            .iter()
            .map(|f| (f.label(), f.value()))
            .collect();
        assert_eq!(
            fields,
            vec![("Email", "dana@example.com"), ("Phone", "555")]
        );
        // The BLE wire format drops the original FieldType, so every
        // reconstructed field defaults to Custom.
        assert!(
            card.fields()
                .iter()
                .all(|f| matches!(f.field_type(), FieldType::Custom)),
            "reconstructed fields must default to FieldType::Custom",
        );
    }
}
