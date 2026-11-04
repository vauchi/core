// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{TransferError, acknowledgement, packet};
use crate::sync::guardian_piece::GuardianBackupPiece;

/// One bounded in-flight piece of the backup named by an accepted share.
pub struct HandBackReceiver {
    hash: [u8; 32],
    metadata: Option<(u16, usize)>,
    bytes: Vec<u8>,
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
        }
    }

    /// Refuses gaps and validates the piece before returning a completed receipt.
    pub fn receive(&mut self, bytes: &[u8]) -> Result<PieceReceipt, TransferError> {
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
        if self.metadata != Some((frame.index, frame.total)) || frame.offset != self.bytes.len() {
            return Err(TransferError::State);
        }
        self.bytes.extend_from_slice(frame.body);
        let piece = if self.bytes.len() == frame.total {
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
}
