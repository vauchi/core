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
    Verify {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
    },
    Confirm {
        session_id: [u8; 16],
        payload_hash: [u8; 32],
    },
    Ready {
        session_id: [u8; 16],
        ack_hash: [u8; 32],
    },
    /// INIT with embedded data: for small payloads (1 chunk), includes the
    /// raw commitment ciphertext. Eliminates the DATA phase entirely.
    /// Peer goes directly from Advertising → has all data in one scan.
    ///
    /// v2 (`IN2D` prefix, this variant): the redundant `pubkey: [u8; 32]`
    /// field has been removed. The v1 `INID` prefix carried both an
    /// `ephemeral` X25519 key and a `pubkey` (a 32-byte identity stand-in)
    /// even though the multi-stage protocol authenticates via the
    /// commitment hash + reveal key, not via the identity pubkey on the
    /// wire. See `_private/docs/problems/2026-05-21-multistage-identity-pubkey-footgun/`.
    Inid {
        session_id: [u8; 16],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        display_name: String,
        relay_url: Option<String>,
        /// Raw commitment ciphertext (not transport-encrypted).
        ciphertext: Vec<u8>,
    },
    /// Compound QR: VRFY + CONF + RDYY in one scan.
    /// Lets a slower peer jump from Transferring → Finalized in a single scan.
    Combo {
        session_id: [u8; 16],
        reveal_key: [u8; 32],
        payload_hash: [u8; 32],
        ack_hash: [u8; 32],
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
            | Self::Verify { session_id, .. }
            | Self::Confirm { session_id, .. }
            | Self::Ready { session_id, .. }
            | Self::Inid { session_id, .. }
            | Self::Combo { session_id, .. }
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
const PREFIXES: [&str; 9] = [
    "INI3", "IN3D", "DAT3", "VRF3", "CNF3", "RDY3", "FAI3", "SHK3", "CMB3",
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
const OLD_FORMAT_PREFIXES: [&str; 9] = [
    "INI2", "IN2D", "DATA", "VRFY", "CONF", "RDYY", "FAIL", "SHAK", "CMBO",
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

#[allow(clippy::too_many_arguments)]
/// Format an INID (INIT+Data) QR for small payloads.
///
/// v2 layout (`IN2D` prefix): same as `format_ini2_qr_with_relay` but
/// with appended ciphertext:
/// `IN2D<sid:24><eph:48><ch:48><name_len:2><name><flags:2>[relay]<ct_len:3><ct>`
///
/// The v1 `INID` layout carried a redundant `<pk:48>` identity stand-in
/// field which has been removed for v2. See
/// `_private/docs/problems/2026-05-21-multistage-identity-pubkey-footgun/`.
///
/// The ciphertext is the raw commitment ciphertext (NOT transport-encrypted).
/// Security: the commitment scheme provides confidentiality (ChaCha20-Poly1305
/// with random reveal key) and integrity (commitment hash). Transport encryption
/// is redundant for single-chunk payloads bound to this session's commitment hash.
#[allow(dead_code)]
pub fn format_in2d_qr(
    session_id: &[u8; 16],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
    relay_url: Option<&str>,
    ciphertext: &[u8],
) -> String {
    assert!(
        display_name.len() <= 99,
        "display_name exceeds 99-byte limit"
    );

    let mut flags: u8 = 0;
    if relay_url.is_some() {
        flags |= FLAG_HAS_RELAY_URL;
    }

    let ct_encoded = base45::encode(ciphertext);

    // Build same as INIT but with INID prefix, then append ciphertext at the end
    let mut result = frame(
        "IN3D",
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

    // Relay fields (same position as INIT)
    if let Some(url) = relay_url {
        result.push_str(&format!("{:03}{}", url.len(), url));
    }

    // Ciphertext appended at the very end with length prefix
    result.push_str(&format!("{:03}{}", ct_encoded.len(), ct_encoded));

    result
}

/// Format a DATA stage QR string with CRC-16 integrity check.
///
/// Layout: `DAT3<header:8><sid:24><idx:4>/<total:4><ack_len:2><ack:variable><crc:3><payload>`
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
        "DAT3",
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

/// Format a VRFY (verify) stage QR string.
pub fn format_verify_qr(session_id: &[u8; 16], reveal_key: &[u8; 32]) -> String {
    frame(
        "VRF3",
        &format!(
            "{sid}{rk}",
            sid = base45::encode(session_id),
            rk = base45::encode(reveal_key),
        ),
    )
}

/// Format a CONF (confirm) stage QR string.
pub fn format_confirm_qr(session_id: &[u8; 16], payload_hash: &[u8; 32]) -> String {
    frame(
        "CNF3",
        &format!(
            "{sid}{ph}",
            sid = base45::encode(session_id),
            ph = base45::encode(payload_hash),
        ),
    )
}

/// Format a READY QR: `RDYY<sid:24><ack_hash:48>`
///
/// The ack_hash is SHA-256(min(sid_a, sid_b) || max(sid_a, sid_b)),
/// proving both sides participated in the same exchange.
/// Kept for backward compatibility with older clients that don't understand CMBO.
#[allow(dead_code)]
pub fn format_ready_qr(session_id: &[u8; 16], ack_hash: &[u8; 32]) -> String {
    frame(
        "RDY3",
        &format!(
            "{sid}{ah}",
            sid = base45::encode(session_id),
            ah = base45::encode(ack_hash),
        ),
    )
}

/// Format a COMBO QR: `CMBO<sid:24><rk:48><ph:48><ah:48>`
///
/// Compound QR containing VRFY reveal_key + CONF payload_hash + RDYY ack_hash.
/// A slower peer can process all three in one scan, jumping from
/// Transferring/Verifying straight to Finalized.
/// Total: 4 + 24 + 48 + 48 + 48 = 172 chars (well within QR capacity).
pub fn format_combo_qr(
    session_id: &[u8; 16],
    reveal_key: &[u8; 32],
    payload_hash: &[u8; 32],
    ack_hash: &[u8; 32],
) -> String {
    frame(
        "CMB3",
        &format!(
            "{sid}{rk}{ph}{ah}",
            sid = base45::encode(session_id),
            rk = base45::encode(reveal_key),
            ph = base45::encode(payload_hash),
            ah = base45::encode(ack_hash),
        ),
    )
}

/// Format a FAIL QR: `FAIL<sid:24>`
///
/// Broadcast to peer so they abort immediately instead of waiting for timeout.
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
pub fn parse_qr(raw: &str) -> Result<StageQr, QrCodecError> {
    parse_frame(raw).map(|frame| frame.stage)
}

/// Parse a QR string into its training header and stage payload.
pub fn parse_frame(raw: &str) -> Result<Frame, QrCodecError> {
    let (prefix, header, body) = split_frame(raw)?;

    let stage = match prefix {
        "INI3" => parse_ini2(body),
        "IN3D" => parse_in2d(body),
        "DAT3" => parse_data(body),
        "VRF3" => parse_verify(body),
        "CNF3" => parse_confirm(body),
        "RDY3" => parse_ready(body),
        "FAI3" => parse_fail(body),
        "SHK3" => parse_shake(body),
        "CMB3" => parse_combo(body),
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

fn parse_in2d(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let eph = take(body, &mut pos, F32_LEN)?;
    let ch = take(body, &mut pos, F32_LEN)?;
    let name_len_str = take(body, &mut pos, NAME_LEN_LEN)?;
    let name_len: usize = name_len_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let name = take(body, &mut pos, name_len)?;
    let flags_encoded = take(body, &mut pos, FLAGS_LEN)?;
    let flags_bytes: [u8; 1] = decode_fixed(flags_encoded)?;
    let flags = flags_bytes[0];

    let relay_url = if flags & FLAG_HAS_RELAY_URL != 0 {
        let url_len_str = take(body, &mut pos, URL_LEN_LEN)?;
        let url_len: usize = url_len_str
            .parse()
            .map_err(|_| QrCodecError::InvalidFieldCount)?;
        let url = take(body, &mut pos, url_len)?;
        Some(url.to_string())
    } else {
        None
    };

    // Ciphertext with length prefix (at the end)
    let ct_len_str = take(body, &mut pos, 3)?;
    let ct_len: usize = ct_len_str
        .parse()
        .map_err(|_| QrCodecError::InvalidFieldCount)?;
    let ct_encoded = take(body, &mut pos, ct_len)?;
    let ciphertext = base45::decode(ct_encoded)?;

    Ok(StageQr::Inid {
        session_id: decode_fixed(sid)?,
        ephemeral: decode_fixed(eph)?,
        commitment_hash: decode_fixed(ch)?,
        display_name: name.to_string(),
        relay_url,
        ciphertext,
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

fn parse_ready(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let ah = take(body, &mut pos, F32_LEN)?;

    Ok(StageQr::Ready {
        session_id: decode_fixed(sid)?,
        ack_hash: decode_fixed(ah)?,
    })
}

fn parse_fail(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;

    Ok(StageQr::Fail {
        session_id: decode_fixed(sid)?,
    })
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

fn parse_combo(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let rk = take(body, &mut pos, F32_LEN)?;
    let ph = take(body, &mut pos, F32_LEN)?;
    let ah = take(body, &mut pos, F32_LEN)?;

    Ok(StageQr::Combo {
        session_id: decode_fixed(sid)?,
        reveal_key: decode_fixed(rk)?,
        payload_hash: decode_fixed(ph)?,
        ack_hash: decode_fixed(ah)?,
    })
}
