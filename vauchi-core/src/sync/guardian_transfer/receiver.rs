// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{TransferError, acknowledgement, packet};
use crate::sync::guardian_piece::GuardianBackupPiece;

/// One bounded guardian-piece buffer for the backup named by an accepted share.
pub struct HandBackReceiver {
    hash: [u8; 32],
    metadata: Option<(u16, usize)>,
    bytes: Vec<u8>,
    cancelled: bool,
}

/// Persist the completed piece before sending its acknowledgement (A11).
#[derive(Debug, PartialEq, Eq)]
pub struct PieceReceipt {
    acknowledgement: Vec<u8>,
    piece: Option<GuardianBackupPiece>,
}

impl PieceReceipt {
    pub fn acknowledgement(&self) -> &[u8] {
        &self.acknowledgement
    }

    pub fn piece(&self) -> Option<&GuardianBackupPiece> {
        self.piece.as_ref()
    }
}

impl HandBackReceiver {
    pub fn new(accepted_backup_hash: [u8; 32]) -> Self {
        Self {
            hash: accepted_backup_hash,
            metadata: None,
            bytes: Vec::new(),
            cancelled: false,
        }
    }

    /// Refuses gaps and validates the piece before returning a completed receipt.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<PieceReceipt, TransferError> {
        if self.cancelled {
            return Err(TransferError::Cancelled);
        }
        let frame = packet(bytes)?;
        let next_piece = self
            .metadata
            .is_some_and(|(index, total)| self.bytes.len() == total && index != frame.index);
        if self.metadata.is_none() || next_piece {
            if frame.offset != 0 {
                return Err(TransferError::State);
            }
            self.metadata = Some((frame.index, frame.total));
            self.bytes = Vec::with_capacity(frame.total);
        }
        if self.metadata != Some((frame.index, frame.total)) {
            return Err(TransferError::State);
        }
        if frame.offset == self.bytes.len() {
            self.bytes.extend_from_slice(frame.body);
        } else if self.bytes.get(frame.offset..frame.end) != Some(frame.body) {
            return Err(TransferError::State);
        }
        let piece = if self.bytes.len() == frame.total && frame.end == frame.total {
            match GuardianBackupPiece::decode(&self.bytes) {
                Ok(piece) if piece.index() == frame.index && piece.hash() == self.hash => {
                    Some(piece)
                }
                _ => {
                    self.bytes.clear();
                    self.metadata = None;
                    return Err(TransferError::PieceMismatch);
                }
            }
        } else {
            None
        };
        Ok(PieceReceipt {
            acknowledgement: acknowledgement(frame.index, frame.end)?,
            piece,
        })
    }

    /// Cancels this receiver permanently; late observations cannot restore a piece.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.metadata = None;
        self.bytes = Vec::new();
    }
}
