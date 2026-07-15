// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport-agnostic device-link join invitation.
//!
//! A `DeviceLinkJoinInvitation` carries everything a fresh device needs to
//! join an existing identity through the relay:
//!
//! - the [`DeviceLinkQR`] data (identity public key + link key + signature)
//! - the relay's rendezvous `broker_code`
//! - an optional explicit relay base URL
//!
//! It is encoded as a shareable URL so the same payload works for QR scan,
//! messaging/email, deep links, and (in the future) BLE broadcast.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};

const MAX_INVITATION_URL_LENGTH: usize = 8 * 1024;
const MAX_ENCODED_RELAY_LENGTH: usize = (crate::relay_url::MAX_URL_LENGTH * 4).div_ceil(3) * 3;

/// Join invitation: public rendezvous data that lets a fresh device claim
/// the initiator's relay slot and decrypt the join response.
///
/// Security: this struct intentionally carries **no secrets**. The master
/// seed is only released by the initiator after the user confirms the
/// confirmation code on the original device. An intercepted invitation lets
/// an attacker post a request, but cannot bypass that confirmation step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceLinkJoinInvitation {
    /// Base64 `DeviceLinkQR` data string (`DeviceLinkQR::to_data_string`).
    pub qr_data: String,
    /// Relay rendezvous code returned by `exchange_offer`.
    pub broker_code: String,
    /// Optional explicit relay base URL. `None` means "use the app's
    /// configured default relay".
    pub relay_url: Option<String>,
}

/// Errors when parsing a join invitation URL.
#[derive(Debug, thiserror::Error)]
pub enum JoinInvitationError {
    /// The URL scheme/path is not a supported invitation format.
    #[error("unsupported invitation URL")]
    UnsupportedUrl,
    /// A required query parameter is missing.
    #[error("missing invitation parameter: {0}")]
    MissingParameter(&'static str),
    /// A parameter contains invalid base64.
    #[error("invalid base64 in invitation parameter {0}: {1}")]
    InvalidBase64(&'static str, String),
    /// A decoded parameter is not valid UTF-8.
    #[error("invalid UTF-8 in invitation parameter {0}")]
    InvalidUtf8(&'static str),
}

impl DeviceLinkJoinInvitation {
    /// Encode the invitation as a `vauchi://device-link` URL.
    ///
    /// Values are URL-safe base64 (no padding) so the URL can be embedded
    /// directly in a QR code or messaging without further escaping.
    pub fn to_url(&self) -> String {
        let qr_b64 = B64.encode(self.qr_data.as_bytes());
        let code_b64 = B64.encode(self.broker_code.as_bytes());
        let mut url = format!("vauchi://device-link?qr={qr_b64}&code={code_b64}");
        if let Some(relay) = &self.relay_url {
            url.push_str(&format!("&relay={}", B64.encode(relay.as_bytes())));
        }
        url
    }

    /// Parse a join invitation from a URL produced by [`Self::to_url`] or
    /// from a future `https://vauchi.app/dl#...` universal-link form.
    pub fn parse_url(url: &str) -> Result<Self, JoinInvitationError> {
        if url.len() > MAX_INVITATION_URL_LENGTH {
            return Err(JoinInvitationError::UnsupportedUrl);
        }

        let rest = if let Some(rest) = strip_invitation_prefix(url) {
            rest
        } else {
            return Err(JoinInvitationError::UnsupportedUrl);
        };

        let query = rest.strip_prefix('?').unwrap_or(rest);
        if query.is_empty() {
            return Err(JoinInvitationError::MissingParameter("qr"));
        }

        let mut qr_raw: Option<&str> = None;
        let mut code_raw: Option<&str> = None;
        let mut relay_raw: Option<&str> = None;

        for pair in query.split('&') {
            let Some((key, value)) = pair.split_once('=') else {
                continue;
            };
            if value.is_empty() {
                continue;
            }
            match key {
                "qr" => qr_raw = Some(value),
                "code" => code_raw = Some(value),
                "relay" => relay_raw = Some(value),
                _ => {}
            }
        }

        let qr_data = decode_required_param(qr_raw, "qr")?;
        let broker_code = decode_required_param(code_raw, "code")?;
        let relay_url = decode_relay_url(relay_raw)?;

        Ok(Self {
            qr_data,
            broker_code,
            relay_url,
        })
    }
}

/// Strip a supported invitation URL prefix and return the query/fragment part.
fn strip_invitation_prefix(url: &str) -> Option<&str> {
    // Native app link forms.
    for prefix in ["vauchi://device-link", "vauchi://device-link/"] {
        if let Some(rest) = url.strip_prefix(prefix)
            && (rest.is_empty() || rest.starts_with('?'))
        {
            return Some(rest);
        }
    }
    // Future universal/web link form: fragment carries the invitation params.
    for prefix in ["https://vauchi.app/dl#", "https://vauchi.app/dl/#"] {
        if let Some(rest) = url.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

/// Decode a required invitation parameter: percent-decode, then base64-decode,
/// then validate UTF-8.
fn decode_required_param(
    value: Option<&str>,
    name: &'static str,
) -> Result<String, JoinInvitationError> {
    let value = value.ok_or(JoinInvitationError::MissingParameter(name))?;
    decode_value(value, name)
}

fn decode_relay_url(value: Option<&str>) -> Result<Option<String>, JoinInvitationError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.len() > MAX_ENCODED_RELAY_LENGTH {
        return Err(JoinInvitationError::UnsupportedUrl);
    }

    let relay_url =
        decode_value(value, "relay").map_err(|_| JoinInvitationError::UnsupportedUrl)?;
    crate::relay_url::validate_relay_url(&relay_url)
        .map_err(|_| JoinInvitationError::UnsupportedUrl)?;
    Ok(Some(relay_url))
}

fn decode_value(value: &str, name: &'static str) -> Result<String, JoinInvitationError> {
    let value = percent_decode(value);
    let bytes = B64
        .decode(value)
        .map_err(|e| JoinInvitationError::InvalidBase64(name, e.to_string()))?;
    String::from_utf8(bytes).map_err(|_| JoinInvitationError::InvalidUtf8(name))
}

/// Minimal percent-decoding for invitation parameter values. Handles `%XX`
/// where `XX` are hex digits; passes everything else through unchanged.
/// This makes the parser tolerant of systems that URL-encode the value
/// before transmission.
///
/// Decoding operates on bytes (not on `char`) so a percent-encoded UTF-8
/// sequence is reconstructed correctly rather than reinterpreted as a
/// Unicode scalar value.
fn percent_decode(value: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            let high = chars.next();
            let low = chars.next();
            if let (Some(h), Some(l)) = (high, low)
                && let Some(byte) = hex_pair_to_byte(h, l)
            {
                bytes.push(byte);
                continue;
            }
            // Malformed escape: preserve the literal bytes so base64 fails cleanly.
            bytes.push(b'%');
            if let Some(h) = high {
                push_char_bytes(&mut bytes, h);
            }
            if let Some(l) = low {
                push_char_bytes(&mut bytes, l);
            }
        } else {
            push_char_bytes(&mut bytes, c);
        }
    }
    bytes
}

fn push_char_bytes(buf: &mut Vec<u8>, c: char) {
    let mut tmp = [0u8; 4];
    buf.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
}

fn hex_pair_to_byte(high: char, low: char) -> Option<u8> {
    let h = high.to_digit(16)?;
    let l = low.to_digit(16)?;
    Some(((h << 4) | l) as u8)
}

// INLINE_TEST_REQUIRED: URL parsing round-trips exercise pub(crate) helpers and error variants.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_invitation() -> DeviceLinkJoinInvitation {
        DeviceLinkJoinInvitation {
            qr_data: "d2hhdGV2ZXI".to_string(),
            broker_code: "BROKER42".to_string(),
            relay_url: None,
        }
    }

    // @internal
    #[test]
    fn roundtrip_url() {
        let inv = sample_invitation();
        let url = inv.to_url();
        let parsed = DeviceLinkJoinInvitation::parse_url(&url).unwrap();
        assert_eq!(parsed, inv);
    }

    // @internal
    #[test]
    fn roundtrip_with_relay_url() {
        let inv = DeviceLinkJoinInvitation {
            qr_data: "d2hhdGV2ZXI".to_string(),
            broker_code: "BROKER42".to_string(),
            relay_url: Some("https://relay.example.com".to_string()),
        };
        let url = inv.to_url();
        let parsed = DeviceLinkJoinInvitation::parse_url(&url).unwrap();
        assert_eq!(parsed, inv);
    }

    // @internal
    #[test]
    fn accepts_slash_after_host() {
        // d2hhdGV2ZXI is base64 for "whatever"; QlJPS0VSNDI for "BROKER42".
        let url = "vauchi://device-link/?qr=d2hhdGV2ZXI&code=QlJPS0VSNDI";
        let parsed = DeviceLinkJoinInvitation::parse_url(url).unwrap();
        assert_eq!(parsed.qr_data, "whatever");
        assert_eq!(parsed.broker_code, "BROKER42");
    }

    // @internal
    #[test]
    fn accepts_future_web_link_fragment() {
        let url = "https://vauchi.app/dl#qr=d2hhdGV2ZXI&code=QlJPS0VSNDI";
        let parsed = DeviceLinkJoinInvitation::parse_url(url).unwrap();
        assert_eq!(parsed.qr_data, "whatever");
        assert_eq!(parsed.broker_code, "BROKER42");
    }

    // @internal
    #[test]
    fn rejects_unsupported_scheme() {
        let result = DeviceLinkJoinInvitation::parse_url("https://evil.com/dl#qr=x&code=y");
        assert!(matches!(result, Err(JoinInvitationError::UnsupportedUrl)));
    }

    // @internal
    #[test]
    fn rejects_missing_qr() {
        let result = DeviceLinkJoinInvitation::parse_url("vauchi://device-link?code=QlJPS0VSNDI");
        assert!(matches!(
            result,
            Err(JoinInvitationError::MissingParameter("qr"))
        ));
    }

    // @internal
    #[test]
    fn rejects_missing_code() {
        let result = DeviceLinkJoinInvitation::parse_url("vauchi://device-link?qr=d2hhdGV2ZXI");
        assert!(matches!(
            result,
            Err(JoinInvitationError::MissingParameter("code"))
        ));
    }

    // @internal
    #[test]
    fn rejects_invalid_base64() {
        let result = DeviceLinkJoinInvitation::parse_url(
            "vauchi://device-link?qr=!!!notbase64!!!&code=QlJPS0VSNDI",
        );
        assert!(matches!(
            result,
            Err(JoinInvitationError::InvalidBase64(_, _))
        ));
    }

    // @internal
    #[test]
    fn ignores_unknown_query_parameters() {
        // Unknown keys are skipped, even if their values are not valid base64.
        let url = "vauchi://device-link?qr=d2hhdGV2ZXI&code=QlJPS0VSNDI&future=abc";
        let parsed = DeviceLinkJoinInvitation::parse_url(url).unwrap();
        assert_eq!(parsed.qr_data, "whatever");
        assert_eq!(parsed.broker_code, "BROKER42");
    }

    // @internal
    #[test]
    fn accepts_percent_encoded_values() {
        // Percent-encode one base64url character ('Z' -> %5A) to verify
        // percent-decoding runs before base64 decoding.
        let url = "vauchi://device-link?qr=d2hhdGV2%5AXI&code=QlJPS0VSNDI";
        let parsed = DeviceLinkJoinInvitation::parse_url(url).unwrap();
        assert_eq!(parsed.qr_data, "whatever");
        assert_eq!(parsed.broker_code, "BROKER42");
    }

    // @internal
    #[test]
    fn accepts_percent_encoded_utf8_in_relay_url() {
        // The relay URL is base64-encoded (ASCII) in the invitation, but a
        // transport may percent-encode those base64 bytes. Reconstructing the
        // original UTF-8 requires byte-level percent-decoding, not char-level.
        let relay = "https://reläy.example.com";
        let relay_b64 = B64.encode(relay.as_bytes());
        let relay_b64_escaped = relay_b64.replacen('l', "%6C", 1).replacen('Q', "%51", 1);
        let url = format!(
            "vauchi://device-link?qr=d2hhdGV2ZXI&code=QlJPS0VSNDI&relay={relay_b64_escaped}"
        );
        let parsed = DeviceLinkJoinInvitation::parse_url(&url).unwrap();
        assert_eq!(parsed.relay_url, Some(relay.to_string()));
    }
}
