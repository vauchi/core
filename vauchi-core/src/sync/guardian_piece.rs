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

/// The most bytes one encoded piece takes, header included; a sealed
/// mailbox message of this size stays far under the relay's 128 KB body
/// limit after encryption and base64.
pub const GUARDIAN_PIECE_MAX_BYTES: usize = 48 * 1024;

/// The most bytes a delivery holds: the guardian backup at its 32 MiB cap
/// plus room for the guardian's sealed share and its framing. Written out
/// because `backup` depends on `sync`, not the reverse (VRS06);
/// `backup::full_backup` asserts at compile time that its cap still fits.
pub const MAX_GUARDIAN_DELIVERY_BYTES: usize = 32 * 1024 * 1024 + 64 * 1024;

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

    /// The SHA-256 of the whole delivery.
    pub fn hash(&self) -> [u8; HASH_BYTES] {
        self.hash
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

/// Splits a guardian's backup into the pieces it sends the recovering
/// phone over Bluetooth (P33). Their id is the start of the backup's hash,
/// not random as for a mailbox delivery, so a resent backup gives the same
/// pieces and those of a retry complete an interrupted transfer.
///
/// # Errors
/// [`PieceError::Size`] for an empty backup or one over
/// [`MAX_GUARDIAN_DELIVERY_BYTES`].
pub fn split_backup_for_hand_back(backup: &[u8]) -> Result<Vec<GuardianBackupPiece>, PieceError> {
    let hash: [u8; HASH_BYTES] = Sha256::digest(backup).into();
    let (backup_id, _) = hash
        .split_first_chunk::<BACKUP_ID_BYTES>()
        .ok_or(PieceError::Size)?;
    split_into_pieces(*backup_id, backup)
}

/// The most bytes a guardian's sealed share takes; a flat or grouped share
/// sealed to one guardian is 124 bytes (Social Recovery, A3).
const MAX_SEALED_SHARE_BYTES: usize = 1024;

/// The most bytes a guardian token takes; one is about 140 bytes.
const MAX_GUARDIAN_TOKEN_BYTES: usize = 1024;

/// Marks the form that carries a guardian token. The earlier form starts
/// with the creation time's high byte, which is zero for any real time.
const WITH_TOKEN_MARKER: u8 = 0x01;

/// What one guardian receives: its own sealed share and the backup every
/// guardian holds (D7), with the designator's time of export, and the
/// guardian's token, the designator's key naming this guardian (D19).
/// Wire form: creation time (big-endian `u64` seconds), share length
/// (big-endian `u16`), the share, then the backup; with a token,
/// `0x01 || time || share length || share || token length (u16) || token
/// || backup`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianDelivery {
    created_at: u64,
    sealed_share: Vec<u8>,
    guardian_token: Option<Vec<u8>>,
    backup: Vec<u8>,
}

impl GuardianDelivery {
    /// # Errors
    /// [`PieceError::Malformed`] for an empty share or one over 1 KiB, or
    /// an empty backup.
    pub fn new(
        created_at: u64,
        sealed_share: Vec<u8>,
        backup: Vec<u8>,
    ) -> Result<Self, PieceError> {
        if !(1..=MAX_SEALED_SHARE_BYTES).contains(&sealed_share.len()) || backup.is_empty() {
            return Err(PieceError::Malformed);
        }
        Ok(Self {
            created_at,
            sealed_share,
            guardian_token: None,
            backup,
        })
    }

    /// The same delivery carrying the guardian's token, as serialized
    /// `GuardianToken` bytes.
    ///
    /// # Errors
    /// [`PieceError::Malformed`] for an empty token or one over 1 KiB.
    pub fn with_guardian_token(mut self, token: Vec<u8>) -> Result<Self, PieceError> {
        if !(1..=MAX_GUARDIAN_TOKEN_BYTES).contains(&token.len()) {
            return Err(PieceError::Malformed);
        }
        self.guardian_token = Some(token);
        Ok(self)
    }

    /// The guardian's token, when the designator sent one.
    pub fn guardian_token(&self) -> Option<&[u8]> {
        self.guardian_token.as_deref()
    }

    /// When the designator exported this backup; a guardian keeps the
    /// newest. Random backup ids and mailbox arrival carry no order, and
    /// the ratchet authenticates the designator who set it.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// The share sealed to this guardian.
    pub fn sealed_share(&self) -> &[u8] {
        &self.sealed_share
    }

    /// The encrypted guardian backup.
    pub fn backup(&self) -> &[u8] {
        &self.backup
    }

    /// The wire form, which [`split_into_pieces`] carries.
    pub fn encode(&self) -> Vec<u8> {
        let share_len = u16::try_from(self.sealed_share.len())
            .expect("a sealed share is at most 1 KiB, checked in new");
        let token_len = self.guardian_token.as_ref().map_or(0, Vec::len);
        let mut bytes = Vec::with_capacity(
            13_usize
                .saturating_add(self.sealed_share.len())
                .saturating_add(token_len)
                .saturating_add(self.backup.len()),
        );
        if self.guardian_token.is_some() {
            bytes.push(WITH_TOKEN_MARKER);
        }
        bytes.extend_from_slice(&self.created_at.to_be_bytes());
        bytes.extend_from_slice(&share_len.to_be_bytes());
        bytes.extend_from_slice(&self.sealed_share);
        if let Some(token) = &self.guardian_token {
            let token_len =
                u16::try_from(token.len()).expect("a token is at most 1 KiB, checked when set");
            bytes.extend_from_slice(&token_len.to_be_bytes());
            bytes.extend_from_slice(token);
        }
        bytes.extend_from_slice(&self.backup);
        bytes
    }

    /// # Errors
    /// [`PieceError::Malformed`] for a short time, a share length of zero,
    /// over 1 KiB or past the end, or no backup after the share.
    pub fn decode(bytes: &[u8]) -> Result<Self, PieceError> {
        let (with_token, bytes) = match bytes.split_first() {
            Some((&WITH_TOKEN_MARKER, rest)) => (true, rest),
            _ => (false, bytes),
        };
        let (created_at, rest) = bytes
            .split_first_chunk::<8>()
            .ok_or(PieceError::Malformed)?;
        let (share_len, rest) = rest.split_first_chunk::<2>().ok_or(PieceError::Malformed)?;
        let (sealed_share, rest) = rest
            .split_at_checked(usize::from(u16::from_be_bytes(*share_len)))
            .ok_or(PieceError::Malformed)?;
        let (token, backup) = if with_token {
            let (token_len, rest) = rest.split_first_chunk::<2>().ok_or(PieceError::Malformed)?;
            let (token, backup) = rest
                .split_at_checked(usize::from(u16::from_be_bytes(*token_len)))
                .ok_or(PieceError::Malformed)?;
            (Some(token.to_vec()), backup)
        } else {
            (None, rest)
        };
        let delivery = Self::new(
            u64::from_be_bytes(*created_at),
            sealed_share.to_vec(),
            backup.to_vec(),
        )?;
        match token {
            Some(token) => delivery.with_guardian_token(token),
            None => Ok(delivery),
        }
    }
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

/// The most bytes an encoded [`HandBackShare`] takes.
pub const MAX_HAND_BACK_SHARE_BYTES: usize = 8 + 2 + MAX_SEALED_SHARE_BYTES + HASH_BYTES;

/// What a guardian shows the recovering phone as a QR code in the meeting
/// (P33): its share re-sealed to the phone's recovery key, and the SHA-256
/// of the backup it sends alongside over Bluetooth, so the phone keeps
/// only pieces a share it took names. Wire form: creation time (big-endian
/// `u64` seconds), share length (big-endian `u16`), the share, the hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandBackShare {
    created_at: u64,
    sealed_share: Vec<u8>,
    backup_hash: [u8; HASH_BYTES],
}

impl HandBackShare {
    /// # Errors
    /// [`PieceError::Malformed`] for an empty share or one over 1 KiB.
    pub fn new(
        created_at: u64,
        sealed_share: Vec<u8>,
        backup_hash: [u8; HASH_BYTES],
    ) -> Result<Self, PieceError> {
        if !(1..=MAX_SEALED_SHARE_BYTES).contains(&sealed_share.len()) {
            return Err(PieceError::Malformed);
        }
        Ok(Self {
            created_at,
            sealed_share,
            backup_hash,
        })
    }

    /// When the owner exported the backup; the phone tries the newest
    /// first.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// The share sealed to the phone's recovery key.
    pub fn sealed_share(&self) -> &[u8] {
        &self.sealed_share
    }

    /// The SHA-256 of the backup the guardian sends over Bluetooth.
    pub fn backup_hash(&self) -> [u8; HASH_BYTES] {
        self.backup_hash
    }

    /// The wire form, shown as one QR code.
    pub fn encode(&self) -> Vec<u8> {
        let share_len = u16::try_from(self.sealed_share.len())
            .expect("a sealed share is at most 1 KiB, checked in new");
        let mut bytes = Vec::with_capacity(MAX_HAND_BACK_SHARE_BYTES);
        bytes.extend_from_slice(&self.created_at.to_be_bytes());
        bytes.extend_from_slice(&share_len.to_be_bytes());
        bytes.extend_from_slice(&self.sealed_share);
        bytes.extend_from_slice(&self.backup_hash);
        bytes
    }

    /// Parses and bounds a scanned share before anything reads it (DC-01).
    ///
    /// # Errors
    /// [`PieceError::Malformed`] for a short time, a share length of zero,
    /// over 1 KiB or past the end, or anything but a hash after the share.
    pub fn decode(bytes: &[u8]) -> Result<Self, PieceError> {
        let (created_at, rest) = bytes
            .split_first_chunk::<8>()
            .ok_or(PieceError::Malformed)?;
        let (share_len, rest) = rest.split_first_chunk::<2>().ok_or(PieceError::Malformed)?;
        let (sealed_share, hash) = rest
            .split_at_checked(usize::from(u16::from_be_bytes(*share_len)))
            .ok_or(PieceError::Malformed)?;
        let backup_hash: [u8; HASH_BYTES] = hash.try_into().map_err(|_| PieceError::Malformed)?;
        Self::new(
            u64::from_be_bytes(*created_at),
            sealed_share.to_vec(),
            backup_hash,
        )
    }
}
