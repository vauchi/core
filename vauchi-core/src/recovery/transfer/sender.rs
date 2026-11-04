// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::{DATA_MAGIC, HEADER_BYTES, MAX_ATTRIBUTE_BYTES, TransferError, read_ack};
use crate::sync::guardian_piece::{GUARDIAN_PIECE_MAX_BYTES, GuardianBackupPiece};

/// One piece, with at most one unacknowledged attribute value.
pub struct HandBackSender {
    bytes: Vec<u8>,
    index: u16,
    offset: usize,
    payload_bytes: usize,
    pending_end: Option<usize>,
}

impl HandBackSender {
    /// Uses the negotiated ATT MTU, bounded to 23..=517.
    pub fn new(piece: &GuardianBackupPiece, mtu: u16) -> Result<Self, TransferError> {
        if !(23..=517).contains(&mtu) {
            return Err(TransferError::Frame);
        }
        let attribute = usize::from(mtu).saturating_sub(3).min(MAX_ATTRIBUTE_BYTES);
        let bytes = piece.encode();
        if bytes.is_empty() || bytes.len() > GUARDIAN_PIECE_MAX_BYTES {
            return Err(TransferError::Frame);
        }
        Ok(Self {
            bytes,
            index: piece.index(),
            offset: 0,
            payload_bytes: attribute
                .checked_sub(HEADER_BYTES)
                .ok_or(TransferError::Frame)?,
            pending_end: None,
        })
    }

    /// Emits one packet, then waits for its acknowledgement.
    pub fn next_packet(&mut self) -> Result<Option<Vec<u8>>, TransferError> {
        if self.pending_end.is_some() || self.is_complete() {
            return Ok(None);
        }
        let end = self
            .offset
            .checked_add(self.payload_bytes)
            .ok_or(TransferError::Frame)?
            .min(self.bytes.len());
        let bytes = self.encode(end)?;
        self.pending_end = Some(end);
        Ok(Some(bytes))
    }

    /// Advances only for the outstanding packet's piece and end offset.
    pub fn acknowledge(&mut self, bytes: &[u8]) -> Result<(), TransferError> {
        let (index, end) = read_ack(bytes)?;
        if index != self.index || self.pending_end != Some(end) {
            return Err(TransferError::State);
        }
        self.offset = end;
        self.pending_end = None;
        Ok(())
    }

    /// Completion concerns this piece's transport, not storage or identity recovery.
    pub fn is_complete(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn encode(&self, end: usize) -> Result<Vec<u8>, TransferError> {
        let body = self
            .bytes
            .get(self.offset..end)
            .ok_or(TransferError::Frame)?;
        let mut packet = DATA_MAGIC.to_vec();
        packet.extend_from_slice(&self.index.to_be_bytes());
        packet.extend_from_slice(
            &u16::try_from(self.offset)
                .map_err(|_| TransferError::Frame)?
                .to_be_bytes(),
        );
        packet.extend_from_slice(
            &u16::try_from(self.bytes.len())
                .map_err(|_| TransferError::Frame)?
                .to_be_bytes(),
        );
        packet.extend_from_slice(body);
        Ok(packet)
    }
}
