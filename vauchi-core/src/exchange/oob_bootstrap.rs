// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! One-sided QR OOB bootstrap payload (Glance).
//!
//! Tier 1 Slice B of `2026-06-10-ble-unauthenticated-peer-identity`. The
//! displayer shows this payload as a QR; the scanner reads it to pin the
//! displayer's identity (`expected_peer`) and to echo a co-presence nonce back
//! inside the authenticated BLE handshake (`oob_nonce_echo`), so a radio-range
//! MITM that never saw the QR cannot complete the exchange (ADR-053).
//!
//! The wire is the shared 174-byte [`super::exchange_payload`] codec under a
//! distinct magic — no second codec (design
//! `2026-06-10-oob-bootstrap-exchange-rituals`). The 16-byte co-presence nonce
//! is HKDF-derived from the signed `token`; see [`derive_oob_nonce`].

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};

use super::ExchangeError;
use super::exchange_payload::{
    EXCHANGE_PAYLOAD_SIZE, ParsedPayload, build_exchange_payload, is_payload_expired,
    parse_exchange_payload, verify_payload_signature,
};
use super::x3dh::X3DHKeyPair;
use crate::crypto::kdf::HKDF;
use crate::identity::Identity;

/// Magic for the Glance one-sided QR OOB bootstrap carrier.
const OOB_QR_MAGIC: &[u8; 4] = b"VOQR";

/// Validity window for a displayed OOB QR (seconds). Matches the BLE handshake
/// expiry — the QR and the radio session share one co-presence window.
pub const OOB_EXPIRY_SECONDS: u64 = 60;

/// Size of the derived OOB co-presence nonce — equals the handshake session's
/// KeyOffer echo width (`NONCE_SIZE`).
pub const OOB_NONCE_SIZE: usize = 16;

/// HKDF domain separation for the OOB nonce (ADR-007).
const OOB_NONCE_DOMAIN: &[u8] = b"vauchi-glance-oob-nonce-v1";

/// Derive the 16-byte OOB co-presence nonce from the QR payload's signed token.
///
/// WHY: the connector proves it *saw* the displayer's QR by echoing this nonce
/// inside the authenticated handshake (ADR-053). For that proof to hold the
/// nonce must be a secret obtainable only from the QR — never from the radio.
/// The 174-byte payload's `token` is fresh-random and signature-covered, and in
/// the Glance flow the payload never crosses BLE (the handshake ships only
/// KeyOffer/KeyAck), so a nonce derived from it stays QR-exclusive. HKDF domain
/// separation keeps it distinct from any other derivation off the same token.
pub fn derive_oob_nonce(token: &[u8; 32]) -> [u8; OOB_NONCE_SIZE] {
    let okm = HKDF::derive_key(None, token, OOB_NONCE_DOMAIN);
    let mut nonce = [0u8; OOB_NONCE_SIZE];
    nonce.copy_from_slice(&okm[..OOB_NONCE_SIZE]);
    nonce
}

/// The Glance one-sided QR OOB bootstrap payload.
///
/// Wraps the shared 174-byte exchange codec under [`OOB_QR_MAGIC`]: the
/// displayer's identity signing key (the scanner's `expected_peer` pin), a
/// fresh ephemeral exchange key, and a signed random token the co-presence
/// nonce derives from.
#[derive(Clone, Debug)]
pub struct OobBootstrapQr {
    inner: ParsedPayload,
}

impl OobBootstrapQr {
    /// Generate a fresh OOB QR for `identity` with a random token at `now`.
    pub fn generate(identity: &Identity, ephemeral: &X3DHKeyPair, now: u64) -> Self {
        let token: [u8; 32] = crate::crypto::random_bytes();
        Self::generate_with_timestamp(identity, ephemeral, token, now)
    }

    /// Generate with an explicit token + timestamp (deterministic / tests).
    pub fn generate_with_timestamp(
        identity: &Identity,
        ephemeral: &X3DHKeyPair,
        token: [u8; 32],
        timestamp: u64,
    ) -> Self {
        let bytes = build_exchange_payload(OOB_QR_MAGIC, identity, ephemeral, token, timestamp);
        let inner = parse_exchange_payload(&bytes, OOB_QR_MAGIC, ExchangeError::InvalidQRFormat)
            .expect("freshly built OOB payload must parse");
        Self { inner }
    }

    /// The displayer's Ed25519 signing key — the scanner's `expected_peer` pin.
    pub fn identity_key(&self) -> &[u8; 32] {
        &self.inner.identity_key
    }

    /// The ephemeral X25519 exchange key.
    pub fn exchange_key(&self) -> &[u8; 32] {
        &self.inner.exchange_key
    }

    /// The signed random token the OOB nonce derives from.
    pub fn token(&self) -> &[u8; 32] {
        &self.inner.token
    }

    /// The 16-byte OOB co-presence nonce (derived from [`Self::token`]).
    pub fn oob_nonce(&self) -> [u8; OOB_NONCE_SIZE] {
        derive_oob_nonce(&self.inner.token)
    }

    /// The generation timestamp (Unix seconds).
    pub fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }

    /// Whether the payload is past its co-presence window at `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        is_payload_expired(self.inner.timestamp, OOB_EXPIRY_SECONDS, now)
    }

    /// Verify the Ed25519 signature against the embedded identity key.
    pub fn verify_signature(&self) -> bool {
        verify_payload_signature(OOB_QR_MAGIC, &self.inner)
    }

    /// Serialize to the 174 raw payload bytes.
    pub fn to_bytes(&self) -> [u8; EXCHANGE_PAYLOAD_SIZE] {
        let mut buf = [0u8; EXCHANGE_PAYLOAD_SIZE];
        buf[0..4].copy_from_slice(OOB_QR_MAGIC);
        buf[4] = self.inner.version;
        buf[5] = self.inner.flags;
        buf[6..38].copy_from_slice(&self.inner.identity_key);
        buf[38..70].copy_from_slice(&self.inner.exchange_key);
        buf[70..102].copy_from_slice(&self.inner.token);
        buf[102..110].copy_from_slice(&self.inner.timestamp.to_be_bytes());
        buf[110..174].copy_from_slice(&self.inner.signature);
        buf
    }

    /// Parse from raw payload bytes — magic/version/length only. Callers MUST
    /// then check [`Self::verify_signature`] + [`Self::is_expired`], or use
    /// [`Self::verified_from_data_string`].
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ExchangeError> {
        // Exact length: the shared codec only rejects *short* buffers, so
        // trailing bytes (outside the signed region) would otherwise be
        // silently dropped, giving one logical QR many valid encodings.
        if bytes.len() != EXCHANGE_PAYLOAD_SIZE {
            return Err(ExchangeError::InvalidQRFormat);
        }
        let inner = parse_exchange_payload(bytes, OOB_QR_MAGIC, ExchangeError::InvalidQRFormat)?;
        Ok(Self { inner })
    }

    /// Encode as the base64 QR string carried by `Component::QrCode`.
    pub fn to_data_string(&self) -> String {
        BASE64.encode(self.to_bytes())
    }

    /// Decode a scanned base64 QR string — parse only (see [`Self::from_bytes`]).
    pub fn from_data_string(data: &str) -> Result<Self, ExchangeError> {
        let bytes = BASE64
            .decode(data.trim())
            .map_err(|_| ExchangeError::InvalidQRFormat)?;
        Self::from_bytes(&bytes)
    }

    /// Decode **and** fully validate a scanned QR: parse, verify signature,
    /// reject if expired at `now`. The scanner path uses this so a tampered or
    /// stale QR can never seed an identity pin or a nonce echo.
    pub fn verified_from_data_string(data: &str, now: u64) -> Result<Self, ExchangeError> {
        let qr = Self::from_data_string(data)?;
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }
        if qr.is_expired(now) {
            return Err(ExchangeError::QRExpired);
        }
        Ok(qr)
    }
}
