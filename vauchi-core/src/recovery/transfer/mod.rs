// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bounded recovery-piece transport (Social Recovery A11, P33).
//! Acknowledgements pace packets; they do not prove persistence or restoration.

mod receiver;
mod sender;

pub use receiver::{HandBackReceiver, PieceReceipt};
pub use sender::HandBackSender;

use crate::sync::guardian_piece::GUARDIAN_PIECE_MAX_BYTES;

const DATA_MAGIC: &[u8; 4] = b"RBP1";
const ACK_MAGIC: &[u8; 4] = b"RBA1";
const HEADER_BYTES: usize = 10;
const MAX_ATTRIBUTE_BYTES: usize = 512;

/// A transfer refused malformed bytes, an invalid sequence or a different backup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum TransferError {
    #[error("Invalid recovery transfer frame")]
    Frame,
    #[error("Unexpected recovery transfer packet")]
    State,
    #[error("Recovery piece does not match the accepted share")]
    PieceMismatch,
}

struct Packet<'a> {
    index: u16,
    offset: usize,
    total: usize,
    end: usize,
    body: &'a [u8],
}

fn number(bytes: &mut &[u8]) -> Result<u16, TransferError> {
    let (value, rest) = bytes.split_first_chunk::<2>().ok_or(TransferError::Frame)?;
    *bytes = rest;
    Ok(u16::from_be_bytes(*value))
}

fn packet(bytes: &[u8]) -> Result<Packet<'_>, TransferError> {
    if bytes.len() > MAX_ATTRIBUTE_BYTES {
        return Err(TransferError::Frame);
    }
    let (magic, mut rest) = bytes.split_first_chunk::<4>().ok_or(TransferError::Frame)?;
    if magic != DATA_MAGIC {
        return Err(TransferError::Frame);
    }
    let index = number(&mut rest)?;
    let offset = usize::from(number(&mut rest)?);
    let total = usize::from(number(&mut rest)?);
    let end = offset.checked_add(rest.len()).ok_or(TransferError::Frame)?;
    if rest.is_empty() || total == 0 || total > GUARDIAN_PIECE_MAX_BYTES || end > total {
        return Err(TransferError::Frame);
    }
    Ok(Packet {
        index,
        offset,
        total,
        end,
        body: rest,
    })
}

fn acknowledgement(index: u16, end: usize) -> Result<Vec<u8>, TransferError> {
    let end = u16::try_from(end).map_err(|_| TransferError::Frame)?;
    let mut bytes = ACK_MAGIC.to_vec();
    bytes.extend_from_slice(&index.to_be_bytes());
    bytes.extend_from_slice(&end.to_be_bytes());
    Ok(bytes)
}

fn read_ack(bytes: &[u8]) -> Result<(u16, usize), TransferError> {
    let (magic, mut rest) = bytes.split_first_chunk::<4>().ok_or(TransferError::Frame)?;
    if magic != ACK_MAGIC {
        return Err(TransferError::Frame);
    }
    let index = number(&mut rest)?;
    let end = usize::from(number(&mut rest)?);
    if !rest.is_empty() || end == 0 || end > GUARDIAN_PIECE_MAX_BYTES {
        return Err(TransferError::Frame);
    }
    Ok((index, end))
}
