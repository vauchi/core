// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Frozen, immutable snapshot of a contact card for use during exchange.
//!
//! A [`CardSnapshot`] captures a [`ContactCard`] at a specific instant so that
//! subsequent mutations to the original card (or the identity layer above) do
//! not affect the data being transmitted mid-exchange.

use serde::{Deserialize, Serialize};

use crate::contact_card::ContactCard;

// ── CardSnapshot type ────────────────────────────────────────────────────────

/// An immutable, timestamped snapshot of a [`ContactCard`].
///
/// Created via [`CardSnapshot::freeze`] which takes ownership of the card and
/// records the current Unix timestamp. The card is then only accessible through
/// the read-only accessor methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardSnapshot {
    card: ContactCard,
    created_at: u64,
}

impl CardSnapshot {
    /// Freeze `card` into an immutable snapshot, recording `now` as the
    /// creation time.
    pub fn freeze(card: ContactCard) -> Self {
        Self {
            card,
            created_at: now_secs(),
        }
    }

    /// Freeze with an explicit timestamp (for testing).
    pub fn freeze_at(card: ContactCard, created_at: u64) -> Self {
        Self { card, created_at }
    }

    /// Returns a reference to the frozen card.
    pub fn card(&self) -> &ContactCard {
        &self.card
    }

    /// Returns the display name from the frozen card.
    pub fn display_name(&self) -> &str {
        self.card.display_name()
    }

    /// Returns the Unix timestamp (seconds) when the snapshot was created.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Serialize the card to JSON bytes.
    ///
    /// Returns the raw UTF-8 JSON encoding of the inner [`ContactCard`], or a
    /// [`serde_json::Error`] if serialization fails.
    /// The `created_at` timestamp is **not** included in the byte output —
    /// callers that need it should store it alongside the bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(&self.card)
    }

    /// Deserialize a snapshot from bytes previously produced by [`Self::to_bytes`].
    ///
    /// The `created_at` timestamp is set to `now` because the original
    /// timestamp is not encoded in the bytes; callers that need round-trip
    /// fidelity should persist the timestamp separately.
    /// Returns a [`serde_json::Error`] if the bytes are not valid JSON.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        let card: ContactCard = serde_json::from_slice(bytes)?;
        Ok(Self {
            card,
            created_at: now_secs(),
        })
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System clock before UNIX epoch")
        .as_secs()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: tests access CardSnapshot internals (card/created_at fields) and
// verify snapshot isolation semantics that require constructing ContactCard directly.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact_card::{ContactCard, ContactField, FieldType};

    #[test]
    fn snapshot_freezes_card() {
        let mut card = ContactCard::new("Alice");
        card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "alice@example.com",
        ))
        .expect("add field");

        let snapshot = CardSnapshot::freeze(card);

        assert_eq!(snapshot.display_name(), "Alice");
        let fields = snapshot.card().fields();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].value(), "alice@example.com");
    }

    #[test]
    fn snapshot_unaffected_by_original_mutation() {
        let mut card = ContactCard::new("Bob");
        card.add_field(ContactField::new(
            FieldType::Email,
            "email",
            "bob@example.com",
        ))
        .expect("add field");

        // Freeze a clone of the card, then mutate the original.
        let snapshot = CardSnapshot::freeze(card.clone());
        // The original card is consumed above; use the clone for mutation.
        let mut mutated = card;
        mutated.set_display_name("Bobby").expect("set name");

        // Snapshot still reflects the state at freeze time.
        assert_eq!(snapshot.display_name(), "Bob");
    }

    #[test]
    fn snapshot_serializes_to_bytes() {
        let card = ContactCard::new("Carol");
        let snapshot = CardSnapshot::freeze(card);

        let bytes = snapshot.to_bytes().unwrap();
        assert!(!bytes.is_empty(), "to_bytes must produce non-empty output");

        let recovered = CardSnapshot::from_bytes(&bytes).expect("from_bytes");
        assert_eq!(recovered.display_name(), "Carol");
    }

    #[test]
    fn snapshot_created_at_is_populated() {
        let card = ContactCard::new("Dave");
        let snapshot = CardSnapshot::freeze(card);
        assert!(
            snapshot.created_at() > 0,
            "created_at should be a positive Unix timestamp"
        );
    }

    #[test]
    fn serde_roundtrip() {
        let card = ContactCard::new("Serde");
        let snapshot = CardSnapshot::freeze(card);
        let json = serde_json::to_string(&snapshot).unwrap();
        let decoded: CardSnapshot = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.display_name(), "Serde");
    }

    #[test]
    fn freeze_at_uses_explicit_timestamp() {
        let card = ContactCard::new("Dana");
        let snapshot = CardSnapshot::freeze_at(card, 1711900000);
        assert_eq!(snapshot.created_at(), 1711900000);
    }
}
