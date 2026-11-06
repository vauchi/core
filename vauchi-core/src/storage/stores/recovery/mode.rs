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

    /// Records when the pending claim was issued.
    pub fn save_recovery_claim_issued_at(&self, issued_at: u64) -> Result<(), StorageError> {
        let issued_at = i64::try_from(issued_at)
            .map_err(|_| StorageError::InvalidData("recovery claim time".into()))?;
        self.conn.execute(
            "UPDATE recovery_mode_key SET claim_issued_at = ?1 WHERE id = 1",
            params![issued_at],
        )?;
        Ok(())
    }

    /// When the pending claim was issued; `None` when no recovery is under
    /// way or the recovery predates this record.
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a negative time.
    pub fn load_recovery_claim_issued_at(&self) -> Result<Option<u64>, StorageError> {
        use rusqlite::OptionalExtension;
        let issued_at: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT claim_issued_at FROM recovery_mode_key WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        issued_at
            .flatten()
            .map(|time| {
                u64::try_from(time)
                    .map_err(|_| StorageError::InvalidData("recovery claim time".into()))
            })
            .transpose()
    }

    /// Discards the hand-backs taken so far (the recovery's target
    /// changed).
    pub fn clear_hand_backs(&self) -> Result<(), StorageError> {
        self.conn.execute("DELETE FROM recovery_hand_backs", [])?;
        self.conn
            .execute("DELETE FROM recovery_backup_pieces", [])?;
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
        self.clear_hand_backs()
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

    /// Keeps one piece of the backup `backup_hash` names, once per index.
    pub fn keep_backup_piece(
        &self,
        backup_hash: &[u8; 32],
        index: u16,
        count: u16,
        piece: &[u8],
        received_at: u64,
    ) -> Result<(), StorageError> {
        let piece_encrypted = crate::crypto::encrypt(self.key, piece)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            "INSERT OR IGNORE INTO recovery_backup_pieces
             (backup_hash, piece_index, piece_count, piece_encrypted, received_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                backup_hash.as_slice(),
                index,
                count,
                piece_encrypted,
                i64::try_from(received_at).unwrap_or(i64::MAX)
            ],
        )?;
        Ok(())
    }

    /// How many pieces of the backup `backup_hash` names are kept, and the
    /// count they give, once one is.
    pub fn backup_piece_progress(
        &self,
        backup_hash: &[u8; 32],
    ) -> Result<Option<(u16, u16)>, StorageError> {
        let (received, count): (i64, Option<i64>) = self.conn.query_row(
            "SELECT COUNT(*), MAX(piece_count) FROM recovery_backup_pieces WHERE backup_hash = ?1",
            params![backup_hash.as_slice()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let Some(count) = count else {
            return Ok(None);
        };
        let narrow = |value: i64| {
            u16::try_from(value).map_err(|_| StorageError::InvalidData("backup piece count".into()))
        };
        Ok(Some((narrow(received)?, narrow(count)?)))
    }

    /// The kept pieces of the backup `backup_hash` names, in order.
    pub fn backup_pieces(&self, backup_hash: &[u8; 32]) -> Result<Vec<Vec<u8>>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT piece_encrypted FROM recovery_backup_pieces
             WHERE backup_hash = ?1 ORDER BY piece_index",
        )?;
        let sealed = stmt
            .query_map(params![backup_hash.as_slice()], |row| {
                row.get::<_, Vec<u8>>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        sealed
            .iter()
            .map(|blob| {
                crate::crypto::decrypt(self.key, blob)
                    .map_err(|e| StorageError::Encryption(e.to_string()))
            })
            .collect()
    }

    /// Drops the pieces of the backup `backup_hash` names, so it can be
    /// sent again.
    pub fn drop_backup_pieces(&self, backup_hash: &[u8; 32]) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM recovery_backup_pieces WHERE backup_hash = ?1",
            params![backup_hash.as_slice()],
        )?;
        Ok(())
    }
}
