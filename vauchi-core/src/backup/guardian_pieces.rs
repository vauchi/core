// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Guardian backup pieces (vauchi/private#796, decision D9).
//!
//! A guardian keeps the designator's encrypted backup next to its shard
//! (D7), but the relay caps a request body at 128 KB while a backup with
//! avatars runs to megabytes. The delivery therefore travels as mailbox
//! messages of at most [`GUARDIAN_PIECE_MAX_BYTES`], each naming its
//! backup, its index and the piece count, and the SHA-256 of the whole
//! delivery. The guardian reassembles them in any order; the hash, checked
//! once every piece is in, refuses a delivery that does not add up.
//!
//! Wire form of a piece: backup id (16 bytes), count and index (big-endian
//! `u16` each), hash (32 bytes), then the body. Every body but the last is
//! exactly [`BODY_MAX_BYTES`] long, so a delivery's size follows from its
//! count and a short inner piece is malformed.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::full_backup::MAX_GUARDIAN_BACKUP_BYTES;

/// The most bytes one encoded piece takes, header included; a sealed
/// mailbox message of this size stays far under the relay's 128 KB body
/// limit after encryption and base64.
pub const GUARDIAN_PIECE_MAX_BYTES: usize = 48 * 1024;

/// The most bytes a delivery holds: the backup at its cap plus room for
/// the guardian's sealed share and its framing.
pub const MAX_GUARDIAN_DELIVERY_BYTES: usize = MAX_GUARDIAN_BACKUP_BYTES + 64 * 1024;

const BACKUP_ID_BYTES: usize = 16;
const HASH_BYTES: usize = 32;
const HEADER_BYTES: usize = BACKUP_ID_BYTES + 2 + 2 + HASH_BYTES;
const BODY_MAX_BYTES: usize = GUARDIAN_PIECE_MAX_BYTES - HEADER_BYTES;
const MAX_PIECES: usize = MAX_GUARDIAN_DELIVERY_BYTES.div_ceil(BODY_MAX_BYTES);

/// Why a delivery could not be split, or a piece not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PieceError {
    /// A delivery must hold between one byte and
    /// [`MAX_GUARDIAN_DELIVERY_BYTES`].
    #[error("A guardian delivery must hold between 1 byte and the size cap")]
    Size,
    /// The bytes are not a well-formed piece.
    #[error("Malformed guardian backup piece")]
    Malformed,
    /// The piece names another backup, count or hash than the pieces
    /// already taken.
    #[error("The piece belongs to another backup")]
    OtherBackup,
    /// A piece arrived again with other bytes.
    #[error("A guardian backup piece arrived twice with different bytes")]
    ConflictingDuplicate,
    /// Every piece is in, and the whole does not match the hash.
    #[error("The reassembled guardian backup does not match its hash")]
    HashMismatch,
}

/// One piece of a guardian delivery; see the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianBackupPiece {
    backup_id: [u8; BACKUP_ID_BYTES],
    count: u16,
    index: u16,
    hash: [u8; HASH_BYTES],
    body: Vec<u8>,
}

impl GuardianBackupPiece {
    /// The backup this piece belongs to.
    pub fn backup_id(&self) -> [u8; BACKUP_ID_BYTES] {
        self.backup_id
    }

    /// This piece's place, from zero.
    pub fn index(&self) -> u16 {
        self.index
    }

    /// How many pieces the delivery has.
    pub fn count(&self) -> u16 {
        self.count
    }

    /// The wire form; see the module doc.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HEADER_BYTES.saturating_add(self.body.len()));
        bytes.extend_from_slice(&self.backup_id);
        bytes.extend_from_slice(&self.count.to_be_bytes());
        bytes.extend_from_slice(&self.index.to_be_bytes());
        bytes.extend_from_slice(&self.hash);
        bytes.extend_from_slice(&self.body);
        bytes
    }

    /// Parses and bounds a piece before anything else reads it (DC-01).
    ///
    /// # Errors
    /// [`PieceError::Malformed`] for a short header, a count of zero or
    /// above the cap, an index past the count, or a body that is not
    /// exactly full (inner piece) or is empty or over-long (last piece).
    pub fn decode(bytes: &[u8]) -> Result<Self, PieceError> {
        let (header, body) = bytes
            .split_at_checked(HEADER_BYTES)
            .ok_or(PieceError::Malformed)?;
        let (backup_id, rest) = header
            .split_first_chunk::<BACKUP_ID_BYTES>()
            .ok_or(PieceError::Malformed)?;
        let (count, rest) = rest.split_first_chunk::<2>().ok_or(PieceError::Malformed)?;
        let (index, hash) = rest.split_first_chunk::<2>().ok_or(PieceError::Malformed)?;
        let hash: [u8; HASH_BYTES] = hash.try_into().map_err(|_| PieceError::Malformed)?;
        let count = u16::from_be_bytes(*count);
        let index = u16::from_be_bytes(*index);

        if count == 0 || usize::from(count) > MAX_PIECES || index >= count {
            return Err(PieceError::Malformed);
        }
        let is_last = count.checked_sub(1) == Some(index);
        let body_fits = if is_last {
            (1..=BODY_MAX_BYTES).contains(&body.len())
        } else {
            body.len() == BODY_MAX_BYTES
        };
        if !body_fits {
            return Err(PieceError::Malformed);
        }

        Ok(Self {
            backup_id: *backup_id,
            count,
            index,
            hash,
            body: body.to_vec(),
        })
    }
}

/// Splits `delivery` into the pieces that carry it to a guardian.
///
/// # Errors
/// [`PieceError::Size`] for an empty delivery or one over
/// [`MAX_GUARDIAN_DELIVERY_BYTES`].
pub fn split_into_pieces(
    backup_id: [u8; BACKUP_ID_BYTES],
    delivery: &[u8],
) -> Result<Vec<GuardianBackupPiece>, PieceError> {
    if delivery.is_empty() || delivery.len() > MAX_GUARDIAN_DELIVERY_BYTES {
        return Err(PieceError::Size);
    }
    let hash: [u8; HASH_BYTES] = Sha256::digest(delivery).into();
    let bodies = delivery.chunks(BODY_MAX_BYTES);
    let count = u16::try_from(bodies.len()).map_err(|_| PieceError::Size)?;

    (0..count)
        .zip(bodies)
        .map(|(index, body)| {
            Ok(GuardianBackupPiece {
                backup_id,
                count,
                index,
                hash,
                body: body.to_vec(),
            })
        })
        .collect()
}

/// Collects the pieces of one delivery until it is whole.
#[derive(Debug, Default)]
pub struct PieceAssembly {
    delivery: Option<([u8; BACKUP_ID_BYTES], u16, [u8; HASH_BYTES])>,
    bodies: BTreeMap<u16, Vec<u8>>,
}

impl PieceAssembly {
    /// Takes one piece; returns the whole delivery once the last missing
    /// piece is in and the whole matches its hash. A piece taken again
    /// with the same bytes changes nothing, so a resent message is
    /// harmless.
    ///
    /// # Errors
    /// [`PieceError::OtherBackup`] for a piece of another delivery,
    /// [`PieceError::ConflictingDuplicate`] for a second copy with other
    /// bytes, [`PieceError::HashMismatch`] when the completed whole does
    /// not match its hash.
    pub fn add(&mut self, piece: GuardianBackupPiece) -> Result<Option<Vec<u8>>, PieceError> {
        let delivery = (piece.backup_id, piece.count, piece.hash);
        if *self.delivery.get_or_insert(delivery) != delivery {
            return Err(PieceError::OtherBackup);
        }
        match self.bodies.get(&piece.index) {
            Some(existing) if *existing != piece.body => {
                return Err(PieceError::ConflictingDuplicate);
            }
            Some(_) => return Ok(None),
            None => {
                self.bodies.insert(piece.index, piece.body);
            }
        }
        if self.bodies.len() < usize::from(piece.count) {
            return Ok(None);
        }

        let whole: Vec<u8> = self.bodies.values().flatten().copied().collect();
        if Sha256::digest(&whole).as_slice() != piece.hash {
            return Err(PieceError::HashMismatch);
        }
        Ok(Some(whole))
    }
}
