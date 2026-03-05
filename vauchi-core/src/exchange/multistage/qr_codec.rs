// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR codec for multi-stage exchange protocol.
//!
//! Formats and parses QR strings for all 4 displayed stages:
//! - `INIT|<session_id>|<pubkey>|<ephemeral>|<commitment_hash>|<display_name>`
//! - `DATA|<session_id>|<chunk_idx>/<total>|<ack_bitmap>|<crc16>|<payload>`
//! - `VRFY|<session_id>|<reveal_key>`
//! - `CONF|<session_id>|<payload_hash>`
//!
//! All binary fields are base45-encoded. The pipe separator (`|`) is NOT in the
//! base45 charset, preventing field-splitting ambiguity.

use super::base45;
use super::crc16;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QrCodecError {
    #[error("unknown QR stage prefix")]
    UnknownPrefix,
    #[error("invalid field count for stage")]
    InvalidFieldCount,
    #[error("base45 decode error: {0}")]
    Base45(#[from] base45::Base45Error),
    #[error("invalid field length: expected {expected}, got {got}")]
    InvalidFieldLength { expected: usize, got: usize },
    #[error("CRC mismatch: expected {expected:#06x}, got {got:#06x}")]
    CrcMismatch { expected: u16, got: u16 },
}

/// Parsed stage QR payload.
#[derive(Debug, Clone, PartialEq)]
pub enum StageQr {
    Init {
        session_id: [u8; 16],
        pubkey: [u8; 32],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        display_name: String,
    },
    Data {
        session_id: [u8; 16],
        chunk_idx: u8,
        chunk_total: u8,
        ack_bitmap: Vec<u8>,
        crc: u16,
        payload: Vec<u8>,
    },
    Verify {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
    },
    Confirm {
        session_id: [u8; 16],
        payload_hash: [u8; 32],
    },
}

/// Field separator — must NOT be in the base45 charset
/// `0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:`
/// to avoid ambiguity when splitting QR strings.
const SEP: char = '|';

fn decode_fixed<const N: usize>(encoded: &str) -> Result<[u8; N], QrCodecError> {
    let bytes = base45::decode(encoded)?;
    if bytes.len() != N {
        return Err(QrCodecError::InvalidFieldLength {
            expected: N,
            got: bytes.len(),
        });
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

/// Format an INIT stage QR string.
pub fn format_init_qr(
    session_id: &[u8; 16],
    pubkey: &[u8; 32],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
) -> String {
    format!(
        "INIT{sep}{sid}{sep}{pk}{sep}{eph}{sep}{ch}{sep}{name}",
        sep = SEP,
        sid = base45::encode(session_id),
        pk = base45::encode(pubkey),
        eph = base45::encode(ephemeral),
        ch = base45::encode(commitment_hash),
        name = display_name,
    )
}

/// Format a DATA stage QR string with CRC-16 integrity check.
pub fn format_data_qr(
    session_id: &[u8; 16],
    chunk_idx: u8,
    chunk_total: u8,
    ack_bitmap: &[u8],
    payload: &[u8],
) -> String {
    let crc = crc16::compute(payload);
    format!(
        "DATA{sep}{sid}{sep}{idx}/{total}{sep}{ack}{sep}{crc}{sep}{data}",
        sep = SEP,
        sid = base45::encode(session_id),
        idx = chunk_idx,
        total = chunk_total,
        ack = base45::encode(ack_bitmap),
        crc = base45::encode(&crc.to_be_bytes()),
        data = base45::encode(payload),
    )
}

/// Format a VRFY (verify) stage QR string.
pub fn format_verify_qr(session_id: &[u8; 16], reveal_key: &[u8; 32]) -> String {
    format!(
        "VRFY{sep}{sid}{sep}{rk}",
        sep = SEP,
        sid = base45::encode(session_id),
        rk = base45::encode(reveal_key),
    )
}

/// Format a CONF (confirm) stage QR string.
pub fn format_confirm_qr(session_id: &[u8; 16], payload_hash: &[u8; 32]) -> String {
    format!(
        "CONF{sep}{sid}{sep}{ph}",
        sep = SEP,
        sid = base45::encode(session_id),
        ph = base45::encode(payload_hash),
    )
}

/// Parse a QR string into a [`StageQr`] variant.
pub fn parse_qr(raw: &str) -> Result<StageQr, QrCodecError> {
    let prefix_end = raw.find(SEP).ok_or(QrCodecError::UnknownPrefix)?;
    let prefix = &raw[..prefix_end];
    let rest = &raw[prefix_end + 1..];

    match prefix {
        "INIT" => parse_init(rest),
        "DATA" => parse_data(rest),
        "VRFY" => parse_verify(rest),
        "CONF" => parse_confirm(rest),
        _ => Err(QrCodecError::UnknownPrefix),
    }
}

fn parse_init(rest: &str) -> Result<StageQr, QrCodecError> {
    let parts: Vec<&str> = rest.splitn(5, SEP).collect();
    if parts.len() != 5 {
        return Err(QrCodecError::InvalidFieldCount);
    }
    Ok(StageQr::Init {
        session_id: decode_fixed(parts[0])?,
        pubkey: decode_fixed(parts[1])?,
        ephemeral: decode_fixed(parts[2])?,
        commitment_hash: decode_fixed(parts[3])?,
        display_name: parts[4].to_string(),
    })
}

fn parse_data(rest: &str) -> Result<StageQr, QrCodecError> {
    let parts: Vec<&str> = rest.splitn(5, SEP).collect();
    if parts.len() != 5 {
        return Err(QrCodecError::InvalidFieldCount);
    }
    let session_id: [u8; 16] = decode_fixed(parts[0])?;

    // Parse "idx/total"
    let idx_parts: Vec<&str> = parts[1].split('/').collect();
    if idx_parts.len() != 2 {
        return Err(QrCodecError::InvalidFieldCount);
    }
    let chunk_idx: u8 = idx_parts[0]
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let chunk_total: u8 = idx_parts[1]
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;

    let ack_bitmap = base45::decode(parts[2])?;
    let crc_bytes: [u8; 2] = decode_fixed(parts[3])?;
    let crc = u16::from_be_bytes(crc_bytes);
    let payload = base45::decode(parts[4])?;

    // Verify CRC
    let computed_crc = crc16::compute(&payload);
    if crc != computed_crc {
        return Err(QrCodecError::CrcMismatch {
            expected: crc,
            got: computed_crc,
        });
    }

    Ok(StageQr::Data {
        session_id,
        chunk_idx,
        chunk_total,
        ack_bitmap,
        crc,
        payload,
    })
}

fn parse_verify(rest: &str) -> Result<StageQr, QrCodecError> {
    let parts: Vec<&str> = rest.splitn(2, SEP).collect();
    if parts.len() != 2 {
        return Err(QrCodecError::InvalidFieldCount);
    }
    Ok(StageQr::Verify {
        session_id: decode_fixed(parts[0])?,
        reveal_key: decode_fixed(parts[1])?,
    })
}

fn parse_confirm(rest: &str) -> Result<StageQr, QrCodecError> {
    let parts: Vec<&str> = rest.splitn(2, SEP).collect();
    if parts.len() != 2 {
        return Err(QrCodecError::InvalidFieldCount);
    }
    Ok(StageQr::Confirm {
        session_id: decode_fixed(parts[0])?,
        payload_hash: decode_fixed(parts[1])?,
    })
}
