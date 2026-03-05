// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR codec for multi-stage exchange protocol.
//!
//! Formats and parses QR strings for all 4 displayed stages using
//! **positional fixed-width** fields (no separator character). This keeps
//! the entire QR payload in the QR alphanumeric charset, resulting in
//! smaller/less-dense QR codes that scan more reliably on front cameras.
//!
//! Layout:
//! - `INIT<sid:24><pk:48><eph:48><ch:48><display_name>`
//! - `DATA<sid:24><idx:3>/<total:3><ack_len:2><ack:variable><crc:3><payload>`
//! - `VRFY<sid:24><rk:48>`
//! - `CONF<sid:24><ph:48>`
//!
//! All binary fields are base45-encoded (fixed-width for known-size inputs).
//! The only non-positional field is `display_name` at the tail of INIT,
//! and `ack`+`payload` in DATA (variable-length, delimited by `ack_len`).

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
    #[error("QR string too short")]
    TooShort,
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

/// Base45-encoded widths for fixed-size binary fields.
const SID_LEN: usize = 24; // base45(16 bytes) = 8 pairs × 3
const F32_LEN: usize = 48; // base45(32 bytes) = 16 pairs × 3
const CRC_LEN: usize = 3; // base45(2 bytes) = 1 pair × 3
const IDX_LEN: usize = 3; // zero-padded decimal "000"–"255"
const ACK_LEN_LEN: usize = 2; // zero-padded decimal length of ack field "00"–"99"

/// Stage prefixes (4 chars each).
const PREFIX_LEN: usize = 4;

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

/// Take `len` chars from `s` at `pos`, advance `pos`.
fn take<'a>(s: &'a str, pos: &mut usize, len: usize) -> Result<&'a str, QrCodecError> {
    if *pos + len > s.len() {
        return Err(QrCodecError::TooShort);
    }
    let slice = &s[*pos..*pos + len];
    *pos += len;
    Ok(slice)
}

/// Take remaining chars from `pos` to end.
fn take_rest(s: &str, pos: usize) -> &str {
    if pos >= s.len() {
        ""
    } else {
        &s[pos..]
    }
}

// ── Formatting ──────────────────────────────────────────────────────────

/// Format an INIT stage QR string (positional, no separators).
pub fn format_init_qr(
    session_id: &[u8; 16],
    pubkey: &[u8; 32],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
) -> String {
    format!(
        "INIT{sid}{pk}{eph}{ch}{name}",
        sid = base45::encode(session_id),
        pk = base45::encode(pubkey),
        eph = base45::encode(ephemeral),
        ch = base45::encode(commitment_hash),
        name = display_name,
    )
}

/// Format a DATA stage QR string with CRC-16 integrity check.
///
/// Layout: `DATA<sid:24><idx:3>/<total:3><ack_len:2><ack:variable><crc:3><payload>`
pub fn format_data_qr(
    session_id: &[u8; 16],
    chunk_idx: u8,
    chunk_total: u8,
    ack_bitmap: &[u8],
    payload: &[u8],
) -> String {
    let crc = crc16::compute(payload);
    let ack_encoded = base45::encode(ack_bitmap);
    format!(
        "DATA{sid}{idx:03}/{total:03}{ack_len:02}{ack}{crc}{data}",
        sid = base45::encode(session_id),
        idx = chunk_idx,
        total = chunk_total,
        ack_len = ack_encoded.len(),
        ack = ack_encoded,
        crc = base45::encode(&crc.to_be_bytes()),
        data = base45::encode(payload),
    )
}

/// Format a VRFY (verify) stage QR string.
pub fn format_verify_qr(session_id: &[u8; 16], reveal_key: &[u8; 32]) -> String {
    format!(
        "VRFY{sid}{rk}",
        sid = base45::encode(session_id),
        rk = base45::encode(reveal_key),
    )
}

/// Format a CONF (confirm) stage QR string.
pub fn format_confirm_qr(session_id: &[u8; 16], payload_hash: &[u8; 32]) -> String {
    format!(
        "CONF{sid}{ph}",
        sid = base45::encode(session_id),
        ph = base45::encode(payload_hash),
    )
}

// ── Parsing ─────────────────────────────────────────────────────────────

/// Parse a QR string into a [`StageQr`] variant.
pub fn parse_qr(raw: &str) -> Result<StageQr, QrCodecError> {
    if raw.len() < PREFIX_LEN {
        return Err(QrCodecError::UnknownPrefix);
    }
    let prefix = &raw[..PREFIX_LEN];
    let body = &raw[PREFIX_LEN..];

    match prefix {
        "INIT" => parse_init(body),
        "DATA" => parse_data(body),
        "VRFY" => parse_verify(body),
        "CONF" => parse_confirm(body),
        _ => Err(QrCodecError::UnknownPrefix),
    }
}

fn parse_init(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let pk = take(body, &mut pos, F32_LEN)?;
    let eph = take(body, &mut pos, F32_LEN)?;
    let ch = take(body, &mut pos, F32_LEN)?;
    let name = take_rest(body, pos);

    Ok(StageQr::Init {
        session_id: decode_fixed(sid)?,
        pubkey: decode_fixed(pk)?,
        ephemeral: decode_fixed(eph)?,
        commitment_hash: decode_fixed(ch)?,
        display_name: name.to_string(),
    })
}

fn parse_data(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;

    // idx(3) + "/" + total(3)
    let idx_str = take(body, &mut pos, IDX_LEN)?;
    let slash = take(body, &mut pos, 1)?;
    if slash != "/" {
        return Err(QrCodecError::InvalidFieldCount);
    }
    let total_str = take(body, &mut pos, IDX_LEN)?;

    let chunk_idx: u8 = idx_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let chunk_total: u8 = total_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;

    // ack_len(2) + ack(variable)
    let ack_len_str = take(body, &mut pos, ACK_LEN_LEN)?;
    let ack_len: usize = ack_len_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let ack_encoded = take(body, &mut pos, ack_len)?;

    // crc(3) + payload(rest)
    let crc_encoded = take(body, &mut pos, CRC_LEN)?;
    let payload_encoded = take_rest(body, pos);

    let ack_bitmap = base45::decode(ack_encoded)?;
    let crc_bytes: [u8; 2] = decode_fixed(crc_encoded)?;
    let crc = u16::from_be_bytes(crc_bytes);
    let payload = base45::decode(payload_encoded)?;

    // Verify CRC
    let computed_crc = crc16::compute(&payload);
    if crc != computed_crc {
        return Err(QrCodecError::CrcMismatch {
            expected: crc,
            got: computed_crc,
        });
    }

    Ok(StageQr::Data {
        session_id: decode_fixed(sid)?,
        chunk_idx,
        chunk_total,
        ack_bitmap,
        crc,
        payload,
    })
}

fn parse_verify(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let rk = take(body, &mut pos, F32_LEN)?;

    Ok(StageQr::Verify {
        session_id: decode_fixed(sid)?,
        reveal_key: decode_fixed(rk)?,
    })
}

fn parse_confirm(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let ph = take(body, &mut pos, F32_LEN)?;

    Ok(StageQr::Confirm {
        session_id: decode_fixed(sid)?,
        payload_hash: decode_fixed(ph)?,
    })
}
