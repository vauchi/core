// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a phone in recovery mode keeps (vauchi/private#796, P32): its
//! temporary recovery key, the identity it recovers, and the hand-backs
//! its guardians give it in person.

use rusqlite::params;

use super::{RecoveryStore, StorageError};

impl RecoveryStore<'_> {
    /// Keeps the temporary recovery key's seed of a phone in recovery mode
    /// and the old identity's key it recovers (P32).
    pub fn save_recovery_mode(
        &self,
        seed: &[u8; 32],
        target_pk: &[u8; 32],
    ) -> Result<(), StorageError> {
        let seed_encrypted = crate::crypto::encrypt(self.key, seed)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR REPLACE INTO recovery_mode_key (id, seed_encrypted, target_pk)
             VALUES (1, ?1, ?2)",
            params![seed_encrypted, target_pk.as_slice()],
        )?;
        Ok(())
    }

    /// The temporary recovery key's seed and the identity it recovers,
    /// while a recovery is under way.
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a malformed row.
    #[allow(clippy::type_complexity)]
    pub fn load_recovery_mode(
        &self,
    ) -> Result<Option<(zeroize::Zeroizing<[u8; 32]>, [u8; 32])>, StorageError> {
        use rusqlite::OptionalExtension;
        let row: Option<(Vec<u8>, Vec<u8>)> = self
            .conn
            .query_row(
                "SELECT seed_encrypted, target_pk FROM recovery_mode_key WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((sealed, target)) = row else {
            return Ok(None);
        };
        let plain = zeroize::Zeroizing::new(
            crate::crypto::decrypt(self.key, &sealed)
                .map_err(|e| StorageError::Encryption(e.to_string()))?,
        );
        let seed = <[u8; 32]>::try_from(plain.as_slice())
            .map_err(|_| StorageError::InvalidData("recovery mode key".into()))?;
        let target = <[u8; 32]>::try_from(target.as_slice())
            .map_err(|_| StorageError::InvalidData("recovery target".into()))?;
        Ok(Some((zeroize::Zeroizing::new(seed), target)))
    }

    /// Discards the hand-backs taken so far (the recovery's target
    /// changed).
    pub fn clear_hand_backs(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM recovery_hand_backs", [])?;
        Ok(())
    }

    /// How many hand-backs are kept.
    pub fn hand_back_count(&self) -> Result<usize, StorageError> {
        let count: i64 =
            self.conn
                .query_row("SELECT COUNT(*) FROM recovery_hand_backs", [], |row| {
                    row.get(0)
                })?;
        Ok(usize::try_from(count).unwrap_or(usize::MAX))
    }

    /// Whether a hand-back with this share digest is kept.
    pub fn has_hand_back(&self, share_digest: &[u8; 32]) -> Result<bool, StorageError> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM recovery_hand_backs WHERE share_digest = ?1",
            params![share_digest.as_slice()],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    /// Discards the temporary recovery key and the hand-backs once the
    /// identity is restored.
    pub fn clear_recovery_mode_seed(&self) -> Result<(), StorageError> {
        self.conn
            .execute("DELETE FROM recovery_mode_key WHERE id = 1", [])?;
        self.conn.execute("DELETE FROM recovery_hand_backs", [])?;
        Ok(())
    }

    /// Keeps a hand-back taken in person, once per share.
    pub fn keep_hand_back(
        &self,
        share_digest: &[u8; 32],
        delivery: &[u8],
        received_at: u64,
    ) -> Result<(), StorageError> {
        let delivery_encrypted = crate::crypto::encrypt(self.key, delivery)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR IGNORE INTO recovery_hand_backs (share_digest, delivery_encrypted, received_at)
             VALUES (?1, ?2, ?3)",
            params![
                share_digest.as_slice(),
                delivery_encrypted,
                i64::try_from(received_at).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// The hand-backs taken so far, as encoded deliveries.
    pub fn hand_backs(&self) -> Result<Vec<Vec<u8>>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT delivery_encrypted FROM recovery_hand_backs ORDER BY rowid")?;
        let sealed = stmt
            .query_map([], |row| row.get::<_, Vec<u8>>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        sealed
            .iter()
            .map(|blob| {
                crate::crypto::decrypt(self.key, blob)
                    .map_err(|e| StorageError::Encryption(e.to_string()))
            })
            .collect()
    }
}
