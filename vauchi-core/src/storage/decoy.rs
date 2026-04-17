// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Decoy Contact Storage
//!
//! CRUD operations for fake contacts displayed during duress mode.
//! Decoy contacts are stored in the `decoy_contacts` table (migration V21).

use rusqlite::params;
use sha2::{Digest, Sha256};

use super::{Storage, StorageError};
use crate::contact_card::ContactCard;

/// `created_at` is backdated by at least this many days so a decoy never looks
/// like it was created the moment duress mode was configured.
const DECOY_MIN_AGE_DAYS: u64 = 30;

/// `created_at` is backdated by at most this many days so timestamps stay within
/// a plausible "active user" range.
const DECOY_MAX_AGE_DAYS: u64 = 365;

/// `updated_at` is backdated to at least this many days ago so even the most
/// recently "touched" decoy looks dormant rather than just-edited.
const DECOY_UPDATE_MIN_AGE_DAYS: u64 = 7;

impl Storage {
    /// Saves a decoy contact.
    ///
    /// The card is encrypted with the storage key before persisting.
    /// Uses INSERT OR REPLACE for idempotent saves.
    pub fn save_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> Result<(), StorageError> {
        let card_json =
            serde_json::to_vec(card).map_err(|e| StorageError::Serialization(e.to_string()))?;

        let encrypted = crate::crypto::encrypt(&self.encryption_key, &card_json)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time before UNIX epoch")
            .as_secs();
        let (created_at, updated_at) = decoy_timestamps(id, now);

        self.conn.execute(
            "INSERT OR REPLACE INTO decoy_contacts (id, display_name, card_encrypted, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, display_name, encrypted, created_at as i64, updated_at as i64],
        )?;

        Ok(())
    }

    /// Loads all decoy contacts.
    ///
    /// Returns a list of (id, display_name, card) tuples.
    pub fn load_decoy_contacts(&self) -> Result<Vec<(String, String, ContactCard)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, display_name, card_encrypted FROM decoy_contacts ORDER BY created_at",
        )?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?;

        let mut contacts = Vec::new();
        for row in rows {
            let (id, display_name, encrypted) = row?;
            let card_json = crate::crypto::decrypt(&self.encryption_key, &encrypted)
                .map_err(|e| StorageError::Encryption(e.to_string()))?;
            let card: ContactCard = serde_json::from_slice(&card_json)
                .map_err(|e| StorageError::Serialization(e.to_string()))?;
            contacts.push((id, display_name, card));
        }

        Ok(contacts)
    }

    /// Deletes a single decoy contact by ID.
    pub fn delete_decoy_contact(&self, id: &str) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM decoy_contacts WHERE id = ?1", params![id])?;

        Ok(())
    }

    /// Deletes all decoy contacts.
    pub fn clear_all_decoy_contacts(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM decoy_contacts", [])?;

        Ok(())
    }
}

/// Derives plausible-looking `(created_at, updated_at)` Unix-second timestamps
/// for a decoy contact, deterministically from its id.
///
/// A coercer or forensic examiner who reads decoy rows must not be able to
/// distinguish them from real contacts by their suspiciously-recent
/// `created_at` (audit finding A2, 2026-04-17). Determinism keeps the same
/// decoy looking the same age across app restarts and re-syncs, so a
/// repeat-inspection of the device does not contradict the first one.
fn decoy_timestamps(id: &str, now: u64) -> (u64, u64) {
    let hash = Sha256::digest(id.as_bytes());
    let take_u64 = |start: usize| -> u64 {
        let bytes: [u8; 8] = hash[start..start + 8]
            .try_into()
            .expect("sha256 output is 32 bytes");
        u64::from_le_bytes(bytes)
    };

    let created_window = (DECOY_MAX_AGE_DAYS - DECOY_MIN_AGE_DAYS) * 86_400;
    let created_offset = take_u64(0) % created_window;
    let created_at = now.saturating_sub(DECOY_MIN_AGE_DAYS * 86_400 + created_offset);

    // updated_at sits between created_at and (now - DECOY_UPDATE_MIN_AGE_DAYS).
    let update_floor = now.saturating_sub(DECOY_UPDATE_MIN_AGE_DAYS * 86_400);
    let update_window = update_floor.saturating_sub(created_at).max(1);
    let update_offset = take_u64(8) % update_window;
    let updated_at = created_at + update_offset;

    (created_at, updated_at)
}

// INLINE_TEST_REQUIRED: tests target the private `decoy_timestamps` helper
#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_now() -> u64 {
        // 2026-04-17 12:00:00 UTC
        1_776_960_000
    }

    // @internal
    #[test]
    fn decoy_timestamps_are_deterministic() {
        let now = fixed_now();
        assert_eq!(
            decoy_timestamps("decoy-1", now),
            decoy_timestamps("decoy-1", now)
        );
    }

    // @internal
    #[test]
    fn decoy_timestamps_differ_per_id() {
        let now = fixed_now();
        let a = decoy_timestamps("alice", now);
        let b = decoy_timestamps("bob", now);
        assert_ne!(a, b);
        assert_ne!(a.0, b.0, "created_at collision between distinct decoys");
    }

    // @internal
    #[test]
    fn decoy_created_at_is_inside_the_documented_window() {
        let now = fixed_now();
        let min_age = DECOY_MIN_AGE_DAYS * 86_400;
        let max_age = DECOY_MAX_AGE_DAYS * 86_400;
        for id in ["a", "bb", "ccc", "dddd", "eeeee", "decoy-7"] {
            let (created, _) = decoy_timestamps(id, now);
            let age = now - created;
            assert!(
                age >= min_age,
                "decoy {id} created_at younger than {DECOY_MIN_AGE_DAYS}d"
            );
            assert!(
                age <= max_age,
                "decoy {id} created_at older than {DECOY_MAX_AGE_DAYS}d"
            );
        }
    }

    // @internal
    #[test]
    fn decoy_updated_at_is_at_least_a_week_old_and_after_created() {
        let now = fixed_now();
        let update_floor = now - DECOY_UPDATE_MIN_AGE_DAYS * 86_400;
        for id in ["a", "decoy-2", "decoy-99"] {
            let (created, updated) = decoy_timestamps(id, now);
            assert!(updated >= created, "updated_at < created_at for {id}");
            assert!(updated <= update_floor, "updated_at too recent for {id}");
        }
    }
}
