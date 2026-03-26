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
    #[error("relay URL present but Noise NK pubkey missing — TOFU not allowed")]
    MissingRelayNoisePubkey,
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
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
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
    Ready {
        session_id: [u8; 16],
        ack_hash: [u8; 32],
    },
    /// INIT with embedded data: for small payloads (1 chunk), includes the
    /// raw commitment ciphertext. Eliminates the DATA phase entirely.
    /// Peer goes directly from Advertising → has all data in one scan.
    Inid {
        session_id: [u8; 16],
        pubkey: [u8; 32],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        display_name: String,
        relay_url: Option<String>,
        relay_noise_pubkey: Option<[u8; 32]>,
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
}

/// Base45-encoded widths for fixed-size binary fields.
const SID_LEN: usize = 24; // base45(16 bytes) = 8 pairs × 3
const F32_LEN: usize = 48; // base45(32 bytes) = 16 pairs × 3
const CRC_LEN: usize = 3; // base45(2 bytes) = 1 pair × 3
const IDX_LEN: usize = 3; // zero-padded decimal "000"–"255"
const ACK_LEN_LEN: usize = 2; // zero-padded decimal length of ack field "00"–"99"

/// Name length field width (zero-padded decimal "00"–"99").
const NAME_LEN_LEN: usize = 2;
/// URL length field width (zero-padded decimal "000"–"999").
const URL_LEN_LEN: usize = 3;
/// Flags field width (base45-encoded 1 byte = 2 chars).
const FLAGS_LEN: usize = 2;

/// Relay metadata flags for INIT QR.
const FLAG_HAS_RELAY_URL: u8 = 0x01;
const FLAG_HAS_RELAY_NOISE_PUBKEY: u8 = 0x02;

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

// ── Formatting ──────────────────────────────────────────────────────────

/// Format an INIT stage QR string with optional relay metadata.
///
/// Layout: `INIT<sid:24><pk:48><eph:48><ch:48><name_len:2><name><flags:3>[<url_len:3><url>][<pubkey:48>]`
///
/// # Panics
///
/// Panics if `display_name` exceeds 99 bytes (2-digit length field)
/// or `relay_url` exceeds 999 bytes (3-digit length field).
pub fn format_init_qr_with_relay(
    session_id: &[u8; 16],
    pubkey: &[u8; 32],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
    relay_url: Option<&str>,
    relay_noise_pubkey: Option<&[u8; 32]>,
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
    if relay_noise_pubkey.is_some() {
        flags |= FLAG_HAS_RELAY_NOISE_PUBKEY;
    }

    let mut result = format!(
        "INIT{sid}{pk}{eph}{ch}{name_len:02}{name}{flags}",
        sid = base45::encode(session_id),
        pk = base45::encode(pubkey),
        eph = base45::encode(ephemeral),
        ch = base45::encode(commitment_hash),
        name_len = display_name.len(),
        name = display_name,
        flags = base45::encode(&[flags]),
    );

    if let Some(url) = relay_url {
        result.push_str(&format!("{:03}{}", url.len(), url));
    }
    if let Some(npk) = relay_noise_pubkey {
        result.push_str(&base45::encode(npk));
    }

    result
}

#[allow(clippy::too_many_arguments)]
/// Format an INID (INIT+Data) QR for small payloads.
///
/// Layout: same as INIT but with prefix `INID` and appended ciphertext.
/// `INID<sid:24><pk:48><eph:48><ch:48><name_len:2><name><flags:2>[relay]<ct_len:3><ct>`
///
/// The ciphertext is the raw commitment ciphertext (NOT transport-encrypted).
/// Security: the commitment scheme provides confidentiality (ChaCha20-Poly1305
/// with random reveal key) and integrity (commitment hash). Transport encryption
/// is redundant for single-chunk payloads bound to this session's commitment hash.
pub fn format_inid_qr(
    session_id: &[u8; 16],
    pubkey: &[u8; 32],
    ephemeral: &[u8; 32],
    commitment_hash: &[u8; 32],
    display_name: &str,
    relay_url: Option<&str>,
    relay_noise_pubkey: Option<&[u8; 32]>,
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
    if relay_noise_pubkey.is_some() {
        flags |= FLAG_HAS_RELAY_NOISE_PUBKEY;
    }

    let ct_encoded = base45::encode(ciphertext);

    // Build same as INIT but with INID prefix, then append ciphertext at the end
    let mut result = format!(
        "INID{sid}{pk}{eph}{ch}{name_len:02}{name}{flags}",
        sid = base45::encode(session_id),
        pk = base45::encode(pubkey),
        eph = base45::encode(ephemeral),
        ch = base45::encode(commitment_hash),
        name_len = display_name.len(),
        name = display_name,
        flags = base45::encode(&[flags]),
    );

    // Relay fields (same position as INIT)
    if let Some(url) = relay_url {
        result.push_str(&format!("{:03}{}", url.len(), url));
    }
    if let Some(npk) = relay_noise_pubkey {
        result.push_str(&base45::encode(npk));
    }

    // Ciphertext appended at the very end with length prefix
    result.push_str(&format!("{:03}{}", ct_encoded.len(), ct_encoded));

    result
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

/// Format a READY QR: `RDYY<sid:24><ack_hash:48>`
///
/// The ack_hash is SHA-256(min(sid_a, sid_b) || max(sid_a, sid_b)),
/// proving both sides participated in the same exchange.
/// Kept for backward compatibility with older clients that don't understand CMBO.
#[allow(dead_code)]
pub fn format_ready_qr(session_id: &[u8; 16], ack_hash: &[u8; 32]) -> String {
    format!(
        "RDYY{sid}{ah}",
        sid = base45::encode(session_id),
        ah = base45::encode(ack_hash),
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
    format!(
        "CMBO{sid}{rk}{ph}{ah}",
        sid = base45::encode(session_id),
        rk = base45::encode(reveal_key),
        ph = base45::encode(payload_hash),
        ah = base45::encode(ack_hash),
    )
}

/// Format a FAIL QR: `FAIL<sid:24>`
///
/// Broadcast to peer so they abort immediately instead of waiting for timeout.
pub fn format_fail_qr(session_id: &[u8; 16]) -> String {
    format!("FAIL{sid}", sid = base45::encode(session_id),)
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
        "INID" => parse_inid(body),
        "DATA" => parse_data(body),
        "VRFY" => parse_verify(body),
        "CONF" => parse_confirm(body),
        "RDYY" => parse_ready(body),
        "FAIL" => parse_fail(body),
        "CMBO" => parse_combo(body),
        _ => Err(QrCodecError::UnknownPrefix),
    }
}

fn parse_init(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let pk = take(body, &mut pos, F32_LEN)?;
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
        #[cfg(any(feature = "network-native-tls", feature = "network-rustls"))]
        crate::network::relay_url::validate_relay_url(url)
            .map_err(|_| QrCodecError::InvalidFieldCount)?;

        Some(url.to_string())
    } else {
        None
    };

    // Optional relay Noise NK pubkey
    let relay_noise_pubkey = if flags & FLAG_HAS_RELAY_NOISE_PUBKEY != 0 {
        let npk = take(body, &mut pos, F32_LEN)?;
        Some(decode_fixed(npk)?)
    } else if relay_url.is_some() {
        // Fail-closed: relay URL without Noise pubkey allows TOFU MITM
        return Err(QrCodecError::MissingRelayNoisePubkey);
    } else {
        None
    };

    Ok(StageQr::Init {
        session_id: decode_fixed(sid)?,
        pubkey: decode_fixed(pk)?,
        ephemeral: decode_fixed(eph)?,
        commitment_hash: decode_fixed(ch)?,
        display_name: name.to_string(),
        relay_url,
        relay_noise_pubkey,
    })
}

fn parse_inid(body: &str) -> Result<StageQr, QrCodecError> {
    let mut pos = 0;
    let sid = take(body, &mut pos, SID_LEN)?;
    let pk = take(body, &mut pos, F32_LEN)?;
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

    let relay_noise_pubkey = if flags & FLAG_HAS_RELAY_NOISE_PUBKEY != 0 {
        let npk = take(body, &mut pos, F32_LEN)?;
        Some(decode_fixed(npk)?)
    } else if relay_url.is_some() {
        return Err(QrCodecError::MissingRelayNoisePubkey);
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
        pubkey: decode_fixed(pk)?,
        ephemeral: decode_fixed(eph)?,
        commitment_hash: decode_fixed(ch)?,
        display_name: name.to_string(),
        relay_url,
        relay_noise_pubkey,
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
