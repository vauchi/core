// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Guardian backups kept for the contacts that designated this device's
//! identity (vauchi/private#796 step 1d, D7).
//!
//! A designator's delivery arrives in mailbox pieces (D9). Each piece is
//! kept, encrypted at rest (ADR-015), until every piece of its backup is
//! in; then the pieces are reassembled against their hash and the delivery
//! replaces the kept one unless it is older. Arrival is counted per backup
//! instead of reassembling on every piece, which would read up to 32 MB per
//! arrival. The caller runs [`GuardianBackupStore::keep_piece`] inside the
//! receive transaction, so a storage failure leaves the blob on the relay.

use rusqlite::{Connection, OptionalExtension, params};

use super::super::{Storage, StorageError};
use crate::crypto::SymmetricKey;
use crate::sync::guardian_piece::{
    GuardianBackupPiece, GuardianDelivery, PieceAssembly, PieceError,
};

/// Unfinished backups kept per owner; one more evicts the oldest, so one
/// contact cannot make this device hold more than two in flight.
const MAX_UNFINISHED_BACKUPS: usize = 2;

/// Where a store keeps its pieces and whole deliveries, and the column that
/// names whose they are. Constants of this module, never input, so they are
/// safe to format into SQL.
struct Tables {
    pieces: &'static str,
    kept: &'static str,
    owner: &'static str,
}

/// Backups this device keeps as a guardian, per designator.
const KEPT_FOR_DESIGNATORS: Tables = Tables {
    pieces: "guardian_backup_pieces",
    kept: "guardian_backups",
    owner: "designator_id",
};

/// Backups guardians returned to this recovering identity, per guardian
/// (step 2a, D13); restore (step 3) reads them.
const RETURNED_BY_GUARDIANS: Tables = Tables {
    pieces: "returned_backup_pieces",
    kept: "returned_backups",
    owner: "guardian_id",
};

/// What [`GuardianBackupStore::keep_piece`] made of a piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptPiece {
    /// Pieces of its backup are still missing.
    Waiting,
    /// The delivery is whole and is now the one kept.
    Kept,
    /// The delivery is whole but older than the one kept, and was dropped.
    Older,
    /// The delivery cannot add up: its pieces were dropped, and the kept
    /// one stays.
    Dropped(PieceError),
}

/// Scoped persistence view for the guardian backups this device keeps.
pub struct GuardianBackupStore<'a> {
    conn: &'a Connection,
    key: &'a SymmetricKey,
    tables: &'static Tables,
}

impl Storage {
    /// Scoped persistence view for the guardian backups this device keeps.
    pub fn guardian_backups(&self) -> GuardianBackupStore<'_> {
        GuardianBackupStore {
            conn: &self.conn,
            key: &self.encryption_key,
            tables: &KEPT_FOR_DESIGNATORS,
        }
    }

    /// Scoped persistence view for the backups guardians returned to this
    /// recovering identity, kept per guardian like the guardian's own.
    pub fn returned_backups(&self) -> GuardianBackupStore<'_> {
        GuardianBackupStore {
            conn: &self.conn,
            key: &self.encryption_key,
            tables: &RETURNED_BY_GUARDIANS,
        }
    }
}

impl GuardianBackupStore<'_> {
    /// Keeps one piece from `owner_id`; see [`KeptPiece`].
    ///
    /// # Errors
    /// Storage and encryption errors; a piece that does not add up is
    /// [`KeptPiece::Dropped`], not an error.
    pub fn keep_piece(
        &self,
        owner_id: &str,
        piece: &GuardianBackupPiece,
        now: u64,
    ) -> Result<KeptPiece, StorageError> {
        let Tables { pieces, owner, .. } = self.tables;
        let backup_id = piece.backup_id();
        let encoded = piece.encode();
        let existing: Option<Vec<u8>> = self
            .conn
            .query_row(
                &format!(
                    "SELECT piece_encrypted FROM {pieces}
                     WHERE {owner} = ?1 AND backup_id = ?2 AND piece_index = ?3"
                ),
                params![owner_id, &backup_id[..], piece.index()],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(sealed) = existing {
            if self.open(&sealed)? == encoded {
                return Ok(KeptPiece::Waiting);
            }
            self.drop_backup(owner_id, &backup_id)?;
            return Ok(KeptPiece::Dropped(PieceError::ConflictingDuplicate));
        }

        if self.held_pieces(owner_id, &backup_id)? == 0 {
            self.make_room(owner_id)?;
        }
        let sealed = crate::crypto::encrypt(self.key, &encoded)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            &format!(
                "INSERT INTO {pieces}
                     ({owner}, backup_id, piece_index, piece_encrypted, received_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![
                owner_id,
                &backup_id[..],
                piece.index(),
                sealed,
                seconds(now)
            ],
        )?;

        if self.held_pieces(owner_id, &backup_id)? < usize::from(piece.count()) {
            return Ok(KeptPiece::Waiting);
        }
        self.complete(owner_id, &backup_id, now)
    }

    /// The newest complete delivery `owner_id` sent, if any.
    ///
    /// # Errors
    /// Storage and decryption errors; [`StorageError::InvalidData`] for a
    /// stored delivery that no longer decodes.
    pub fn kept(&self, owner_id: &str) -> Result<Option<GuardianDelivery>, StorageError> {
        let Tables { kept, owner, .. } = self.tables;
        let sealed: Option<Vec<u8>> = self
            .conn
            .query_row(
                &format!("SELECT delivery_encrypted FROM {kept} WHERE {owner} = ?1"),
                params![owner_id],
                |row| row.get(0),
            )
            .optional()?;
        sealed
            .map(|sealed| {
                GuardianDelivery::decode(&self.open(&sealed)?)
                    .map_err(|e| StorageError::InvalidData(e.to_string()))
            })
            .transpose()
    }

    /// How many backups of `owner_id` have pieces waiting.
    ///
    /// # Errors
    /// Storage errors.
    pub fn unfinished_backups(&self, owner_id: &str) -> Result<usize, StorageError> {
        let Tables { pieces, owner, .. } = self.tables;
        let count: i64 = self.conn.query_row(
            &format!("SELECT COUNT(DISTINCT backup_id) FROM {pieces} WHERE {owner} = ?1"),
            params![owner_id],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Forgets the kept backup and the waiting pieces `designator_id` left,
    /// on its release (vauchi/private#796).
    ///
    /// # Errors
    /// Storage errors.
    pub fn forget_designator(&self, designator_id: &str) -> Result<(), StorageError> {
        let Tables {
            pieces,
            kept,
            owner,
        } = self.tables;
        self.conn.execute(
            &format!("DELETE FROM {pieces} WHERE {owner} = ?1"),
            params![designator_id],
        )?;
        self.conn.execute(
            &format!("DELETE FROM {kept} WHERE {owner} = ?1"),
            params![designator_id],
        )?;
        Ok(())
    }

    /// The content digest and time of this designator's last send to its
    /// guardians, if it ever sent one (step 1e).
    ///
    /// # Errors
    /// Storage errors; [`StorageError::InvalidData`] for a digest that is
    /// not 32 bytes.
    pub fn last_sent(&self) -> Result<Option<([u8; 32], u64)>, StorageError> {
        let row: Option<(Vec<u8>, i64)> = self
            .conn
            .query_row(
                "SELECT content_digest, sent_at FROM guardian_backup_sent WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(digest, sent_at)| {
            let digest: [u8; 32] = digest
                .try_into()
                .map_err(|_| StorageError::InvalidData("guardian backup digest".into()))?;
            Ok((digest, u64::try_from(sent_at).unwrap_or(0)))
        })
        .transpose()
    }

    /// Records what this designator just sent its guardians.
    ///
    /// # Errors
    /// Storage errors.
    pub fn record_sent(&self, digest: &[u8; 32], sent_at: u64) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO guardian_backup_sent (id, content_digest, sent_at) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET
                 content_digest = excluded.content_digest,
                 sent_at = excluded.sent_at",
            params![&digest[..], seconds(sent_at)],
        )?;
        Ok(())
    }

    fn complete(
        &self,
        owner_id: &str,
        backup_id: &[u8; 16],
        now: u64,
    ) -> Result<KeptPiece, StorageError> {
        let Tables {
            pieces,
            kept,
            owner,
        } = self.tables;
        let mut statement = self.conn.prepare(&format!(
            "SELECT piece_encrypted FROM {pieces}
             WHERE {owner} = ?1 AND backup_id = ?2 ORDER BY piece_index"
        ))?;
        let sealed_pieces = statement
            .query_map(params![owner_id, &backup_id[..]], |row| {
                row.get::<_, Vec<u8>>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut assembly = PieceAssembly::default();
        let mut whole = None;
        for sealed in sealed_pieces {
            let piece = GuardianBackupPiece::decode(&self.open(&sealed)?)
                .map_err(|e| StorageError::InvalidData(e.to_string()))?;
            match assembly.add(piece) {
                Ok(done) => whole = done.or(whole),
                Err(error) => {
                    self.drop_backup(owner_id, backup_id)?;
                    return Ok(KeptPiece::Dropped(error));
                }
            }
        }
        self.drop_backup(owner_id, backup_id)?;
        let Some(bytes) = whole else {
            return Ok(KeptPiece::Dropped(PieceError::Malformed));
        };
        let delivery = match GuardianDelivery::decode(&bytes) {
            Ok(delivery) => delivery,
            Err(error) => return Ok(KeptPiece::Dropped(error)),
        };

        let kept_at: Option<i64> = self
            .conn
            .query_row(
                &format!("SELECT created_at FROM {kept} WHERE {owner} = ?1"),
                params![owner_id],
                |row| row.get(0),
            )
            .optional()?;
        if kept_at.is_some_and(|kept_at| seconds(delivery.created_at()) < kept_at) {
            return Ok(KeptPiece::Older);
        }
        let sealed = crate::crypto::encrypt(self.key, &bytes)
            .map_err(|e| StorageError::Encryption(e.to_string()))?;
        self.conn.execute(
            &format!(
                "INSERT INTO {kept} ({owner}, created_at, delivery_encrypted, received_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT({owner}) DO UPDATE SET
                     created_at = excluded.created_at,
                     delivery_encrypted = excluded.delivery_encrypted,
                     received_at = excluded.received_at"
            ),
            params![
                owner_id,
                seconds(delivery.created_at()),
                sealed,
                seconds(now)
            ],
        )?;
        Ok(KeptPiece::Kept)
    }

    fn held_pieces(&self, owner_id: &str, backup_id: &[u8; 16]) -> Result<usize, StorageError> {
        let Tables { pieces, owner, .. } = self.tables;
        let count: i64 = self.conn.query_row(
            &format!("SELECT COUNT(*) FROM {pieces} WHERE {owner} = ?1 AND backup_id = ?2"),
            params![owner_id, &backup_id[..]],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Evicts the oldest unfinished backups until a new one fits.
    fn make_room(&self, owner_id: &str) -> Result<(), StorageError> {
        let Tables { pieces, owner, .. } = self.tables;
        while self.unfinished_backups(owner_id)? >= MAX_UNFINISHED_BACKUPS {
            let oldest: Vec<u8> = self.conn.query_row(
                &format!(
                    "SELECT backup_id FROM {pieces} WHERE {owner} = ?1
                     GROUP BY backup_id ORDER BY MIN(received_at), MIN(rowid) LIMIT 1"
                ),
                params![owner_id],
                |row| row.get(0),
            )?;
            self.conn.execute(
                &format!("DELETE FROM {pieces} WHERE {owner} = ?1 AND backup_id = ?2"),
                params![owner_id, oldest],
            )?;
        }
        Ok(())
    }

    fn drop_backup(&self, owner_id: &str, backup_id: &[u8; 16]) -> Result<(), StorageError> {
        let Tables { pieces, owner, .. } = self.tables;
        self.conn.execute(
            &format!("DELETE FROM {pieces} WHERE {owner} = ?1 AND backup_id = ?2"),
            params![owner_id, &backup_id[..]],
        )?;
        Ok(())
    }

    fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, StorageError> {
        crate::crypto::decrypt(self.key, sealed)
            .map_err(|e| StorageError::Encryption(e.to_string()))
    }
}

/// SQLite keeps integers signed; a time past `i64::MAX` seconds is clamped.
fn seconds(time: u64) -> i64 {
    i64::try_from(time).unwrap_or(i64::MAX)
}
