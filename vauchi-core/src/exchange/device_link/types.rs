// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Constants, enums, and helper functions for the device linking protocol.

use aws_lc_rs::hmac;

use crate::crypto::HKDF;

/// QR code magic bytes for device linking.
pub(super) const DEVICE_LINK_MAGIC: &[u8; 4] = b"WBDL";

/// Protocol version for device linking.
pub(super) const DEVICE_LINK_VERSION: u8 = 1;

/// Link QR expiration time in seconds.
///
/// Production default: 300s (5 minutes).
/// With `test-timings` feature: 5s (for fast e2e tests).
#[cfg(not(feature = "test-timings"))]
pub(super) const LINK_QR_EXPIRY_SECONDS: u64 = 300;
#[cfg(feature = "test-timings")]
pub(super) const LINK_QR_EXPIRY_SECONDS: u64 = 5;

/// Domain separator for deriving the proximity challenge from the link key.
pub(super) const PROXIMITY_DOMAIN: &[u8] = b"vauchi-device-link-proximity-v1";

/// Maximum age (seconds) of a proximity proof before it's considered expired.
pub(super) const PROXIMITY_PROOF_MAX_AGE_SECS: u64 = 60;

/// Domain separator for confirmation code MAC.
pub(super) const CONFIRMATION_MAC_DOMAIN: &[u8] = b"vauchi-device-link-confirm-mac-v1";

/// Evidence of proximity verification.
///
/// Platforms must construct this from real session data — not a bare boolean.
/// Core validates the proof before releasing the master seed.
#[derive(Debug, Clone)]
pub enum ProximityProof {
    /// Ultrasonic challenge-response completed successfully.
    Ultrasonic {
        /// The 16-byte challenge response received from the other device.
        challenge_response: [u8; 16],
        /// Unix timestamp (seconds) when verification completed.
        verified_at: u64,
    },
    /// Manual confirmation: user compared codes on both screens.
    /// Weaker than ultrasonic but time-bound and session-bound.
    ManualConfirmation {
        /// HMAC-SHA256(derived_key, confirmation_code) — proves the caller
        /// had access to the real confirmation code from this specific session.
        confirmation_code_mac: [u8; 32],
        /// Unix timestamp (seconds) when user confirmed.
        confirmed_at: u64,
    },
}

/// Confirmation details shown to the initiating device before approving a link.
///
/// Both devices independently compute the same confirmation code. The user
/// compares the codes on both screens before approving. This prevents a remote
/// attacker who intercepts QR data from silently linking their device.
#[derive(Debug, Clone)]
pub struct DeviceLinkConfirmation {
    /// The new device's proposed name.
    pub device_name: String,
    /// 6-digit confirmation code (formatted as `XXX-XXX`), derived from shared
    /// material so both devices display the same code.
    pub confirmation_code: String,
    /// Identity fingerprint (first 8 bytes of identity public key, hex-formatted
    /// with separators: `AB12-CD34-EF56-7890`).
    pub identity_fingerprint: String,
}

/// Compute HMAC-SHA256 for manual confirmation proof.
///
/// Uses domain-separated key derivation: HKDF(link_key, domain) → HMAC key.
/// Public so that platform layers can construct a valid `ManualConfirmation` proof.
pub fn compute_confirmation_mac(link_key: &[u8; 32], confirmation_code: &str) -> [u8; 32] {
    let derived_key = HKDF::derive(None, link_key, CONFIRMATION_MAC_DOMAIN, 32)
        .expect("32 bytes is valid HKDF output length");
    let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, &derived_key);
    let tag = hmac::sign(&hmac_key, confirmation_code.as_bytes());
    let mut mac = [0u8; 32];
    mac.copy_from_slice(tag.as_ref());
    mac
}

/// Generates a 6-digit numeric code from cryptographically random bytes.
///
/// This serves as a fallback pairing mechanism when QR code scanning is not
/// available (e.g., accessibility needs, camera failure). The code is derived
/// from secure random bytes and formatted as XXX-XXX.
pub fn generate_numeric_code() -> String {
    let bytes: [u8; 4] = crate::crypto::random_bytes();
    let value = u32::from_be_bytes(bytes) % 1_000_000;
    format!("{:03}-{:03}", value / 1000, value % 1000)
}

/// Derives a 16-byte proximity challenge from the link key using HKDF.
///
/// Both sides of the device linking protocol can derive the same challenge
/// deterministically, since they share the link key (from the QR code).
pub(super) fn derive_proximity_challenge(link_key: &[u8; 32]) -> [u8; 16] {
    let derived = HKDF::derive(None, link_key, PROXIMITY_DOMAIN, 16)
        .expect("16 bytes is valid HKDF output length");
    let mut challenge = [0u8; 16];
    challenge.copy_from_slice(&derived);
    challenge
}

/// Derives a 6-digit confirmation code from the link key and request nonce.
///
/// Both the initiator and responder can compute this independently since they
/// share the link key (from QR) and the nonce (from the request). Uses
/// HMAC-SHA256 with the link key as the signing key and the nonce as the
/// message, then takes the first 3 bytes modulo 1_000_000 for a 6-digit code.
pub(super) fn derive_confirmation_code(link_key: &[u8; 32], request_nonce: &[u8; 32]) -> String {
    let hmac_key = hmac::Key::new(hmac::HMAC_SHA256, link_key);
    let tag = hmac::sign(&hmac_key, request_nonce);
    let bytes = tag.as_ref();

    // Take first 4 bytes as u32, reduce to 6 digits
    let value = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) % 1_000_000;
    format!("{:03}-{:03}", value / 1000, value % 1000)
}
