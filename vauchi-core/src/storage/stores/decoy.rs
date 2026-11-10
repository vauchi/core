// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Decoy contacts: rows of the mask profile's `contacts`, each with the decoy
//! id it was added under, which the decoy API is keyed by (ADR-080 point 4,
//! amended 2026-11-08).
//!
//! A decoy's contact id is the hex of SHA-256 of its decoy id, as the duress
//! session has always shown it.

use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};

use super::super::{Sealed, Storage, StorageError};
use crate::card::ContactCard;
use crate::contact::Contact;
use crate::crypto::{SymmetricKey, decrypt, encrypt};

/// Scoped persistence view for decoy contacts.
pub struct DecoyStore<'a> {
    mask: &'a Storage,
    /// Sealed after each write; a write whose seal fails returns the error
    /// (vauchi/private#1003).
    sealer: &'a Storage,
}

impl Storage {
    /// The mask profile's decoy contacts.
    pub fn decoy(&self) -> Sealed<'_, DecoyStore<'_>> {
        let mask = self.mask_profile();
        self.mask_sealed(mask.sealed(DecoyStore { mask, sealer: self }))
    }
}

/// The signing key a decoy's contact is stored under.
pub(crate) fn decoy_public_key(decoy_id: &str) -> [u8; 32] {
    Sha256::digest(decoy_id.as_bytes()).into()
}

impl DecoyStore<'_> {
    fn key(&self) -> &SymmetricKey {
        &self.mask.encryption_key
    }

    /// Saves a decoy contact, backdated so it never looks freshly made.
    pub fn save_decoy_contact(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
    ) -> Result<(), StorageError> {
        let (created_at, updated_at) = decoy_timestamps(id, self.mask.clock.unix_seconds());
        self.insert_decoy(id, display_name, card, created_at, updated_at)
    }

    /// Saves a decoy contact with the timestamps it already has.
    pub(crate) fn insert_decoy(
        &self,
        id: &str,
        display_name: &str,
        card: &ContactCard,
        created_at: u64,
        updated_at: u64,
    ) -> Result<(), StorageError> {
        let contact = Contact::from_exchange(
            decoy_public_key(id),
            card.clone(),
            SymmetricKey::generate(),
            created_at,
        );
        self.mask.contacts().save_contact(&contact)?;
        let name = encrypt(self.key(), display_name.as_bytes())
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.mask.conn.execute(
            "INSERT OR REPLACE INTO decoy_ids
             (contact_id, decoy_id, display_name_encrypted, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![contact.id(), id, name, created_at as i64, updated_at as i64],
        )?;
        self.sealer.seal_mask()
    }

    /// Loads all decoy contacts, oldest first, as (id, display_name, card).
    pub fn load_decoy_contacts(&self) -> Result<Vec<(String, String, ContactCard)>, StorageError> {
        let mut stmt = self.mask.conn.prepare(
            "SELECT contact_id, decoy_id, display_name_encrypted FROM decoy_ids
             ORDER BY created_at",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut decoys = Vec::with_capacity(rows.len());
        for (contact_id, decoy_id, name) in rows {
            let name = String::from_utf8(
                decrypt(self.key(), &name).map_err(|e| StorageError::Encryption(e.to_string()))?,
            )
            .map_err(|e| StorageError::Serialization(e.to_string()))?;
            let Some(contact) = self.mask.contacts().load_contact(&contact_id)? else {
                continue;
            };
            decoys.push((decoy_id, name, contact.card().clone()));
        }
        Ok(decoys)
    }

    /// Deletes a single decoy contact by its decoy id.
    pub fn delete_decoy_contact(&self, id: &str) -> Result<(), StorageError> {
        let contact_id: Option<String> = self
            .mask
            .conn
            .query_row(
                "SELECT contact_id FROM decoy_ids WHERE decoy_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(contact_id) = contact_id {
            self.mask.delete_contact(&contact_id)?;
            self.mask.conn.execute(
                "DELETE FROM decoy_ids WHERE contact_id = ?1",
                params![contact_id],
            )?;
            self.sealer.seal_mask()?;
        }
        Ok(())
    }

    /// Deletes all decoy contacts.
    pub fn clear_all_decoy_contacts(&self) -> Result<(), StorageError> {
        let ids: Vec<String> = self
            .mask
            .conn
            .prepare("SELECT decoy_id FROM decoy_ids")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for id in ids {
            self.delete_decoy_contact(&id)?;
        }
        Ok(())
    }
}

/// `created_at` is backdated by at least this many days so a decoy never looks
/// like it was created the moment duress mode was configured.
const DECOY_MIN_AGE_DAYS: u64 = 30;

/// `created_at` is backdated by at most this many days so timestamps stay within
/// a plausible "active user" range.
const DECOY_MAX_AGE_DAYS: u64 = 365;

/// `updated_at` is backdated to at least this many days ago so even the most
/// recently "touched" decoy looks dormant rather than just-edited.
const DECOY_UPDATE_MIN_AGE_DAYS: u64 = 7;

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

    // @scenario: duress_mode :: Decoy profile has realistic timestamps
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

    // @scenario: duress_mode :: Decoy profile has realistic timestamps
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
