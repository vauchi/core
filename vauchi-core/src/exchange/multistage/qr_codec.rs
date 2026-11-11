// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
//! - `RVL4<header:8><sid:24><rk:48>`
//! - `CNF4<header:8><sid:24><rk:48><tag:48>`
//! - `DON4<header:8><sid:24><rk:48><tag:48>`, the confirm once saved
//! - `BTH4<header:8><sid:24><rk:48><tag:48>`, DONE's tag once the peer's
//!   DONE was read
//!
//! All binary fields are base45-encoded (fixed-width for known-size inputs).
//! The only non-positional field is `display_name` at the tail of INIT,
//! and `ack`+`payload` in DATA (variable-length, delimited by `ack_len`).

use super::base45;
use super::crc16;
use super::training_header::{HEADER_LEN, TrainingHeader};
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
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
    /// A frame in the format used before link training: the sender needs
    /// an update. Distinct from a QR that is not ours at all.
    #[error("frame is in the previous format")]
    OldFormat,
    #[error("invalid training header")]
    InvalidHeader,
}

/// A parsed frame: its training header and its stage payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub header: TrainingHeader,
    pub stage: StageQr,
}

/// Parsed stage QR payload.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StageQr {
    Init {
        session_id: [u8; 16],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        display_name: String,
        relay_url: Option<String>,
    },
    Data {
        session_id: [u8; 16],
        chunk_idx: u16,
        chunk_total: u16,
        ack_bitmap: Vec<u8>,
        crc: u16,
        payload: Vec<u8>,
    },
    /// REVEAL: the sender's reveal key, shown until its checks of the peer's
    /// payload passed; no tag yet (vauchi/private#1217).
    Reveal {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
    },
    /// CONFIRM: the reveal key and a tag keyed from the transport key
    /// saying the sender checked and accepted the reader's payload. One
    /// read takes a peer that holds every chunk and has checked the sender
    /// to Finalized.
    Final {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
        tag: [u8; 32],
    },
    /// DONE: shown by a phone that has saved, with its own keyed tag. A
    /// peer that has not finalized reads it as a confirm; a saved peer
    /// learns that both have (design D6).
    Done {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
        tag: [u8; 32],
    },
    /// BOTH: shown by a saved phone that read its peer's DONE. It carries
    /// the sender's DONE tag, so the reader that stops at once on it, maybe
    /// before reading a DONE, still knows the peer saved (owner decision
    /// 2026-11-11, vauchi/private#1217; unkeyed since 2026-10-04 until then).
    Both {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
        tag: [u8; 32],
    },
    /// Failure notification — tells peer to abort immediately.
    Fail { session_id: [u8; 16] },
    /// SHAK stage: an AEAD-sealed accelerometer magnitude envelope for the
    /// TapHoverShake "shake" co-location cross-correlation (advisory signal).
    ///
    /// `sealed_envelope` is **opaque ciphertext** (`nonce || ct+tag` from
    /// [`super::accel_envelope::seal_envelope`]); the codec only frames it and
    /// CRC-checks for QR-scan integrity — it never inspects or decrypts the
    /// bytes. Opened by [`super::accel_envelope::open_envelope`] in the
    /// orchestrator. Carries no protocol-state transition (ADR-009 amendment).
    Shake {
        session_id: [u8; 16],
        sealed_envelope: Vec<u8>,
    },
}

impl StageQr {
    /// The session id of the phone that drew this frame.
    pub fn session_id(&self) -> &[u8; 16] {
        match self {
            Self::Init { session_id, .. }
            | Self::Data { session_id, .. }
            | Self::Final { session_id, .. }
            | Self::Done { session_id, .. }
            | Self::Both { session_id, .. }
            | Self::Reveal { session_id, .. }
            | Self::Fail { session_id }
            | Self::Shake { session_id, .. } => session_id,
        }
    }
}

/// Base45-encoded widths for fixed-size binary fields.
const SID_LEN: usize = 24; // base45(16 bytes) = 8 pairs × 3
const F32_LEN: usize = 48; // base45(32 bytes) = 16 pairs × 3
const CRC_LEN: usize = 3; // base45(2 bytes) = 1 pair × 3
const IDX_LEN: usize = 4; // zero-padded decimal "0000"–"9999"
const ACK_LEN_LEN: usize = 2; // length of the ack field, two base45 digits
/// Longest ack field two base45 digits can count: a bitmap for 10 792
/// chunks, far past the 32 KB card limit at any chunk size.
const MAX_ACK_LEN: usize = 44 * 45 + 44;

/// Name length field width (zero-padded decimal "00"–"99").
const NAME_LEN_LEN: usize = 2;
/// URL length field width (zero-padded decimal "000"–"999").
const URL_LEN_LEN: usize = 3;
/// Flags field width (base45-encoded 1 byte = 2 chars).
const FLAGS_LEN: usize = 2;

/// Relay metadata flags for INIT QR.
const FLAG_HAS_RELAY_URL: u8 = 0x01;

/// Stage prefixes (4 chars each).
const PREFIX_LEN: usize = 4;
const PREFIXES: [&str; 8] = [
    "INI3", "RVL4", "DAT4", "CNF4", "DON4", "BTH4", "FAI3", "SHK3",
];

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
///
/// Uses `str::get()` instead of direct indexing to avoid panic if the
/// slice boundary falls inside a multi-byte UTF-8 codepoint.
fn take<'a>(s: &'a str, pos: &mut usize, len: usize) -> Result<&'a str, QrCodecError> {
    if *pos + len > s.len() {
        return Err(QrCodecError::TooShort);
    }
    let slice = s.get(*pos..*pos + len).ok_or(QrCodecError::TooShort)?;
    *pos += len;
    Ok(slice)
}

/// Take remaining chars from `pos` to end.
fn take_rest(s: &str, pos: usize) -> &str {
    s.get(pos..).unwrap_or("")
}

/// Join a stage prefix and its body into a frame. Every `format_*` goes
/// through here, and every parse through [`split_frame`].
fn frame(prefix: &str, body: &str) -> String {
    debug_assert_eq!(prefix.len(), PREFIX_LEN);
    format!("{prefix}{}{body}", TrainingHeader::default().encode())
}

/// Stage prefixes of the format used before every frame carried a header.
const OLD_FORMAT_PREFIXES: [&str; 14] = [
    "INI2", "IN2D", "DATA", "VRFY", "CONF", "RDYY", "FAIL", "SHAK", "CMBO",
    // DAT3 chunks did not authenticate the ACK they carried (#676).
    "DAT3",
    // IN3D carried the commitment ciphertext outside transport encryption,
    // and FIN3/DON3 an unkeyed tag: together they let a recorded payload be
    // replayed in another session (#1217).
    "IN3D", "FIN3", "DON3",
    // BTH3 carried no tag, so it could not prove the peer saved (#1217).
    "BTH3",
];

/// Split a frame into its stage prefix, training header and body.
fn split_frame(raw: &str) -> Result<(&str, TrainingHeader, &str), QrCodecError> {
    // `split_at_checked`, not slicing: any QR a camera sees lands here, and
    // slicing panics when a boundary falls inside a multi-byte character.
    let (prefix, rest) = raw
        .split_at_checked(PREFIX_LEN)
        .ok_or(QrCodecError::UnknownPrefix)?;
    if OLD_FORMAT_PREFIXES.contains(&prefix) {
        return Err(QrCodecError::OldFormat);
    }
    if !PREFIXES.contains(&prefix) {
        return Err(QrCodecError::UnknownPrefix);
    }
    let (header, body) = rest
        .split_at_checked(HEADER_LEN)
        .ok_or(QrCodecError::TooShort)?;
    Ok((prefix, TrainingHeader::parse(header)?, body))
}

/// The same frame carrying `header` in place of the one it has.
pub fn with_header(frame: &str, header: &TrainingHeader) -> Result<String, QrCodecError> {
    let (prefix, _, body) = split_frame(frame)?;
    Ok(format!("{prefix}{}{body}", header.encode()))
}

/// Format an INIT stage QR string with optional relay metadata.
///
/// v2 layout (`INI2` prefix): `INI2<sid:24><eph:48><ch:48><name_len:2><name><flags:3>[<url_len:3><url>][<pubkey:48>]`
///
/// The v1 `INIT` layout carried a redundant 48-char `<pk:48>` between
/// `<sid>` and `<eph>` — a 32-byte identity stand-in that the multi-stage
/// protocol does not authenticate against (auth is via commitment hash +
/// reveal key). The field was removed for v2; see
/// `_private/docs/problems/2026-05-21-multistage-identity-pubkey-footgun/`.
///
/// # Panics
///
/// Panics if `display_name` exceeds 99 bytes (2-digit length field)
/// or `relay_url` exceeds 999 bytes (3-digit length field).
pub fn format_ini2_qr_with_relay(
    session_id: &[u8; 16],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
    relay_url: Option<&str>,
) -> String {
    assert!(
        display_name.len() <= 99,
        "display_name exceeds 99-byte limit for 2-digit length field"
    );
    if let Some(url) = relay_url {
        assert!(
            url.len() <= 999,
            "relay_url exceeds 999-byte limit for 3-digit length field"
        );
    }

    let mut flags: u8 = 0;
    if relay_url.is_some() {
        flags |= FLAG_HAS_RELAY_URL;
    }

    let mut result = frame(
        "INI3",
        &format!(
            "{sid}{eph}{ch}{name_len:02}{name}{flags}",
            sid = base45::encode(session_id),
            eph = base45::encode(ephemeral),
            ch = base45::encode(commitment_hash),
            name_len = display_name.len(),
            name = display_name,
            flags = base45::encode(&[flags]),
        ),
    );

    if let Some(url) = relay_url {
        result.push_str(&format!("{:03}{}", url.len(), url));
    }

    result
}

/// Format a DATA stage QR string with CRC-16 integrity check.
///
/// Layout: `DAT4<header:8><sid:24><idx:4>/<total:4><ack_len:2><ack:variable><crc:3><payload>`
///
/// # Panics
///
/// Panics if the encoded ack bitmap is longer than 2024 characters.
pub fn format_data_qr(
    session_id: &[u8; 16],
    chunk_idx: u16,
    chunk_total: u16,
    ack_bitmap: &[u8],
    payload: &[u8],
) -> String {
    let crc = crc16::compute(payload);
    let ack_encoded = base45::encode(ack_bitmap);
    assert!(
        ack_encoded.len() <= MAX_ACK_LEN,
        "ack bitmap exceeds what the two-digit length field can count"
    );
    frame(
        "DAT4",
        &format!(
            "{sid}{idx:04}/{total:04}{ack_len_high}{ack_len_low}{ack}{crc}{data}",
            sid = base45::encode(session_id),
            idx = chunk_idx,
            total = chunk_total,
            ack_len_high = base45::digit((ack_encoded.len() / 45) as u8),
            ack_len_low = base45::digit((ack_encoded.len() % 45) as u8),
            ack = ack_encoded,
            crc = base45::encode(&crc.to_be_bytes()),
            data = base45::encode(payload),
        ),
    )
}

/// Format a REVEAL frame: `RVL4<header:8><sid:24><rk:48>`, shown until the
/// sender's checks of the peer's payload passed (#1217).
pub fn format_reveal_qr(session_id: &[u8; 16], reveal_key: &[u8; 32]) -> String {
    frame(
        "RVL4",
        &format!(
            "{sid}{rk}",
            sid = base45::encode(session_id),
            rk = base45::encode(reveal_key),
        ),
    )
}

/// Format a CONFIRM frame: `CNF4<header:8><sid:24><rk:48><tag:48>`, 132
/// characters, the density of the opening frame. The tag is keyed (#1217).
pub fn format_final_qr(session_id: &[u8; 16], reveal_key: &[u8; 32], tag: &[u8; 32]) -> String {
    frame("CNF4", &final_body(session_id, reveal_key, tag))
}

/// Format a DONE frame: the confirm's layout under `DON4`, with its own
/// keyed tag, shown once the sender has saved.
pub fn format_done_qr(session_id: &[u8; 16], reveal_key: &[u8; 32], tag: &[u8; 32]) -> String {
    frame("DON4", &final_body(session_id, reveal_key, tag))
}

/// Format a BOTH frame: DONE's layout and tag under `BTH4`.
pub fn format_both_qr(session_id: &[u8; 16], reveal_key: &[u8; 32], tag: &[u8; 32]) -> String {
    frame("BTH4", &final_body(session_id, reveal_key, tag))
}

fn final_body(session_id: &[u8; 16], reveal_key: &[u8; 32], tag: &[u8; 32]) -> String {
    format!(
        "{sid}{rk}{tag}",
        sid = base45::encode(session_id),
        rk = base45::encode(reveal_key),
        tag = base45::encode(tag),
    )
}

/// Format a FAIL frame: `FAI3<header:8><sid:24>`. A phone that rejected
/// its peer's payload, or could not save it, shows it so the peer stops at
/// once (vauchi/private#1217).
pub fn format_fail_qr(session_id: &[u8; 16]) -> String {
    frame("FAI3", &base45::encode(session_id))
}

/// Format a SHAK (shake-envelope) stage QR string with CRC-16 integrity check.
///
/// Layout: `SHAK<sid:24><crc:3><sealed(base45)>`. `sealed_envelope` is the
/// opaque AEAD ciphertext from [`super::accel_envelope::seal_envelope`]; the
/// CRC covers those sealed bytes (a scan-integrity check distinct from — and
/// in addition to — the AEAD tag).
pub fn format_shake_qr(session_id: &[u8; 16], sealed_envelope: &[u8]) -> String {
    let crc = crc16::compute(sealed_envelope);
    frame(
        "SHK3",
        &format!(
            "{sid}{crc}{env}",
            sid = base45::encode(session_id),
            crc = base45::encode(&crc.to_be_bytes()),
            env = base45::encode(sealed_envelope),
        ),
    )
}

/// Parse a QR string into its stage payload, dropping the training header.
/// The session parses with [`parse_frame`]; tests use this.
#[allow(dead_code)]
pub fn parse_qr(raw: &str) -> Result<StageQr, QrCodecError> {
    parse_frame(raw).map(|frame| frame.stage)
}

/// Parse a QR string into its training header and stage payload.
pub fn parse_frame(raw: &str) -> Result<Frame, QrCodecError> {
    let (prefix, header, body) = split_frame(raw)?;

    let stage = match prefix {
        "INI3" => parse_ini2(body),
        "RVL4" => parse_reveal(body),
        "DAT4" => parse_data(body),
        "CNF4" => parse_final_body(body).map(|(session_id, reveal_key, tag)| StageQr::Final {
            session_id,
            reveal_key,
            tag,
        }),
        "DON4" => parse_final_body(body).map(|(session_id, reveal_key, tag)| StageQr::Done {
            session_id,
            reveal_key,
            tag,
        }),
        "BTH4" => parse_final_body(body).map(|(session_id, reveal_key, tag)| StageQr::Both {
            session_id,
            reveal_key,
            tag,
        }),
        "FAI3" => parse_fail(body),
        "SHK3" => parse_shake(body),
        _ => Err(QrCodecError::UnknownPrefix),
    }?;
    Ok(Frame { header, stage })
}

fn parse_ini2(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let eph = take(body, &mut pos, F32_LEN)?;
    let ch = take(body, &mut pos, F32_LEN)?;

    // Name with length prefix
    let name_len_str = take(body, &mut pos, NAME_LEN_LEN)?;
    let name_len: usize = name_len_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let name = take(body, &mut pos, name_len)?;

    // Flags byte
    let flags_encoded = take(body, &mut pos, FLAGS_LEN)?;
    let flags_bytes: [u8; 1] = decode_fixed(flags_encoded)?;
    let flags = flags_bytes[0];

    // Optional relay URL
    let relay_url = if flags & FLAG_HAS_RELAY_URL != 0 {
        let url_len_str = take(body, &mut pos, URL_LEN_LEN)?;
        let url_len: usize = url_len_str
            .parse()
            .map_err(|_| QrCodecError::InvalidFieldCount)?;
        let url = take(body, &mut pos, url_len)?;

        // SSRF prevention: validate relay URL at parse time
        #[cfg(feature = "network-rustls")]
        crate::relay_url::validate_relay_url(url).map_err(|_| QrCodecError::InvalidFieldCount)?;

        Some(url.to_string())
    } else {
        None
    };

    // Optional relay Noise NK pubkey
    Ok(StageQr::Init {
        session_id: decode_fixed(sid)?,
        ephemeral: decode_fixed(eph)?,
        commitment_hash: decode_fixed(ch)?,
        display_name: name.to_string(),
        relay_url,
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

    let chunk_idx: u16 = idx_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let chunk_total: u16 = total_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;

    // ack_len(2) + ack(variable)
    let ack_len = match take(body, &mut pos, ACK_LEN_LEN)?.as_bytes() {
        [high, low] => {
            usize::from(base45::digit_value(*high)?) * 45 + usize::from(base45::digit_value(*low)?)
        }
        _ => return Err(QrCodecError::InvalidFieldCount),
    };
    let ack_encoded = take(body, &mut pos, ack_len)?;

    // crc(3) + payload(rest)
    let crc_encoded = take(body, &mut pos, CRC_LEN)?;
    let payload_encoded = take_rest(body, pos);

    let ack_bitmap = base45::decode(ack_encoded)?;
    let crc_bytes: [u8; 2] = decode_fixed(crc_encoded)?;
    let crc = u16::from_be_bytes(crc_bytes);
    let payload = base45::decode(payload_encoded)?;

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

type FinalBody = ([u8; 16], [u8; 32], [u8; 32]);

fn parse_final_body(body: &str) -> Result<FinalBody, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let rk = take(body, &mut pos, F32_LEN)?;
    let tag = take(body, &mut pos, F32_LEN)?;

    Ok((decode_fixed(sid)?, decode_fixed(rk)?, decode_fixed(tag)?))
}

fn parse_reveal(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let rk = take(body, &mut pos, F32_LEN)?;
    Ok(StageQr::Reveal {
        session_id: decode_fixed(sid)?,
        reveal_key: decode_fixed(rk)?,
    })
}

fn parse_session_id_only(body: &str) -> Result<[u8; 16], QrCodecError> {
    let mut pos = 0;
    decode_fixed(take(body, &mut pos, SID_LEN)?)
}

fn parse_fail(body: &str) -> Result<StageQr, QrCodecError> {
    parse_session_id_only(body).map(|session_id| StageQr::Fail { session_id })
}

fn parse_shake(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let crc_encoded = take(body, &mut pos, CRC_LEN)?;
    let env_encoded = take_rest(body, pos);

    let crc_bytes: [u8; 2] = decode_fixed(crc_encoded)?;
    let crc = u16::from_be_bytes(crc_bytes);
    let sealed_envelope = base45::decode(env_encoded)?;

    // Scan-integrity check over the sealed bytes (the AEAD tag is verified
    // later by `accel_envelope::open_envelope`; this catches QR-scan flips
    // earlier, matching the DATA stage's convention).
    let computed_crc = crc16::compute(&sealed_envelope);
    if crc != computed_crc {
        return Err(QrCodecError::CrcMismatch {
            expected: crc,
            got: computed_crc,
        });
    }

    Ok(StageQr::Shake {
        session_id: decode_fixed(sid)?,
        sealed_envelope,
    })
}
