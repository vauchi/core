// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared Exchange Payload
//!
//! Common 174-byte payload format used by NFC Active and BLE exchanges.
//! Both use identical binary layout, differing only in magic bytes and expiry.
//!
//! Layout (174 bytes):
//!   Magic (4) | Version (1) | Flags (1) | Identity Key (32) |
//!   Exchange Key (32) | Token (32) | Timestamp (8) | Signature (64)

use super::ExchangeError;
use crate::crypto::{PublicKey, Signature};
use crate::identity::Identity;

use super::x3dh::X3DHKeyPair;

/// Common payload size for NFC and BLE exchange payloads.
pub const EXCHANGE_PAYLOAD_SIZE: usize = 174;

/// Parsed exchange payload — transport-agnostic.
#[derive(Clone, Debug)]
pub struct ParsedPayload {
    pub version: u8,
    pub flags: u8,
    pub identity_key: [u8; 32],
    pub exchange_key: [u8; 32],
    pub token: [u8; 32],
    pub timestamp: u64,
    pub signature: [u8; 64],
}

/// Builds a 174-byte exchange payload.
///
/// Used by both `ExchangeNfc` and `ExchangeBle` — they differ only
/// in `magic` bytes and expiry duration.
pub fn build_exchange_payload(
    magic: &[u8; 4],
    identity: &Identity,
    ephemeral: &X3DHKeyPair,
    token: [u8; 32],
    timestamp: u64,
) -> [u8; EXCHANGE_PAYLOAD_SIZE] {
    let identity_key = *identity.signing_public_key();
    let exchange_key = *ephemeral.public_key();
    let flags = 0u8;
    let version = 1u8;

    let message = build_sign_message(
        magic,
        version,
        flags,
        &identity_key,
        &exchange_key,
        &token,
        timestamp,
    );
    let signature = identity.sign(&message);

    let mut buf = [0u8; EXCHANGE_PAYLOAD_SIZE];
    buf[0..4].copy_from_slice(magic);
    buf[4] = version;
    buf[5] = flags;
    buf[6..38].copy_from_slice(&identity_key);
    buf[38..70].copy_from_slice(&exchange_key);
    buf[70..102].copy_from_slice(&token);
    buf[102..110].copy_from_slice(&timestamp.to_be_bytes());
    buf[110..174].copy_from_slice(signature.as_bytes());

    buf
}

/// Parses a 174-byte exchange payload, checking magic and version.
///
/// Does NOT check signature or expiry — callers verify those separately.
pub fn parse_exchange_payload(
    bytes: &[u8],
    expected_magic: &[u8; 4],
    format_error: ExchangeError,
) -> Result<ParsedPayload, ExchangeError> {
    if bytes.len() < EXCHANGE_PAYLOAD_SIZE {
        return Err(format_error.clone());
    }

    if &bytes[0..4] != expected_magic {
        return Err(format_error);
    }

    let version = bytes[4];
    if version != 1 {
        return Err(ExchangeError::InvalidProtocolVersion);
    }

    let flags = bytes[5];

    let mut identity_key = [0u8; 32];
    identity_key.copy_from_slice(&bytes[6..38]);

    let mut exchange_key = [0u8; 32];
    exchange_key.copy_from_slice(&bytes[38..70]);

    let mut token = [0u8; 32];
    token.copy_from_slice(&bytes[70..102]);

    let timestamp = u64::from_be_bytes(
        bytes[102..110]
            .try_into()
            .map_err(|_| ExchangeError::InvalidNfcFormat)?,
    );

    let mut signature = [0u8; 64];
    signature.copy_from_slice(&bytes[110..174]);

    Ok(ParsedPayload {
        version,
        flags,
        identity_key,
        exchange_key,
        token,
        timestamp,
        signature,
    })
}

/// Checks if a payload timestamp has expired given the expiry duration.
pub fn is_payload_expired(timestamp: u64, expiry_secs: u64) -> bool {
    let now = super::now_secs();

    now > timestamp + expiry_secs
}

/// Verifies the Ed25519 signature on a parsed payload.
pub fn verify_payload_signature(magic: &[u8; 4], payload: &ParsedPayload) -> bool {
    let message = build_sign_message(
        magic,
        payload.version,
        payload.flags,
        &payload.identity_key,
        &payload.exchange_key,
        &payload.token,
        payload.timestamp,
    );

    let public_key = PublicKey::from_bytes(payload.identity_key);
    let signature = Signature::from_bytes(payload.signature);

    public_key.verify(&message, &signature)
}

fn build_sign_message(
    magic: &[u8; 4],
    version: u8,
    flags: u8,
    identity_key: &[u8; 32],
    exchange_key: &[u8; 32],
    token: &[u8; 32],
    timestamp: u64,
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(4 + 1 + 1 + 32 + 32 + 32 + 8);
    msg.extend_from_slice(magic);
    msg.push(version);
    msg.push(flags);
    msg.extend_from_slice(identity_key);
    msg.extend_from_slice(exchange_key);
    msg.extend_from_slice(token);
    msg.extend_from_slice(&timestamp.to_be_bytes());
    msg
}
