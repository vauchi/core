// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link mode: asynchronous exchange via URL + relay escrow.
//!
//! The initiator generates a URL containing an ephemeral X25519 public key
//! and a random nonce. The responder opens the URL, performs ECDH, and
//! deposits their card. Both sides derive escrow hashes from the shared
//! secret using [`EscrowKeys`].
//!
//! Core produces [`ExchangeCommand`]s — frontends execute relay calls and
//! report results via `ExchangeHardwareEvent`s (ADR-031).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::exchange::command::ExchangeCommand;
use crate::exchange::escrow::{EscrowKeys, EscrowRole};

/// Default TTL for escrow deposits (7 days, matching protocol max).
const DEFAULT_TTL_SECONDS: u32 = 604_800;

/// URL scheme for link exchange.
const LINK_URL_PREFIX: &str = "vauchi://exchange";

/// Error from Link mode operations.
#[derive(Debug, thiserror::Error)]
pub enum LinkModeError {
    /// The peer's public key is a small-order point (non-contributory DH).
    #[error("non-contributory Diffie-Hellman output (small-order point)")]
    NonContributoryDh,
    /// The peer's public key has invalid length (expected 32 bytes).
    #[error("malformed peer public key: expected 32 bytes, got {received}")]
    MalformedPeerKey { received: usize },
    /// Card encryption or decryption failed.
    #[error("card crypto failed: {0}")]
    CardCryptoFailed(String),
    /// No card data available to send.
    #[error("no card snapshot available for exchange")]
    NoCardToSend,
}

// =========================================================================
// Initiator
// =========================================================================

/// Result of initiator URL generation.
pub struct LinkInitiation {
    /// The URL to share (contains ephemeral public key + nonce).
    pub url: String,
    /// Nonce (needed for handshake slot derivation on poll).
    pub nonce: [u8; 32],
    /// Ephemeral secret key bytes (zeroized on drop). Must be persisted
    /// for later ECDH when the responder's public key arrives.
    pub secret_key_bytes: Zeroizing<[u8; 32]>,
    /// Handshake gate hash (hex) — gate where initiator presence + responder epk are deposited.
    pub handshake_slot: String,
    /// Initiator's presence slot hash (hex) within the handshake gate.
    /// Used for GET authentication when retrieving the responder's epk.
    pub presence_slot: String,
}

/// Generate a Link mode URL, ephemeral keypair, and presence deposit command.
///
/// Returns the initiation data and a command to deposit the initiator's
/// presence to the handshake gate. The presence deposit must be executed
/// before sharing the URL — it creates the initiator's slot in the
/// handshake gate so that GET works after the responder deposits their epk.
///
/// The caller must persist `secret_key_bytes` and `nonce` for later
/// use in [`initiator_complete`].
pub fn initiator_generate() -> (LinkInitiation, Vec<ExchangeCommand>) {
    // Use StaticSecret (not EphemeralSecret) because we need to persist
    // the secret key bytes for later ECDH. StaticSecret supports to_bytes().
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let nonce: [u8; 32] = crate::crypto::random_bytes();

    let pk_b64 = URL_SAFE_NO_PAD.encode(public.as_bytes());
    let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce);
    let url = format!("{LINK_URL_PREFIX}?pk={pk_b64}&n={nonce_b64}");

    let handshake_slot = derive_handshake_slot(&nonce);
    let presence_slot = derive_initiator_presence_slot(&nonce);
    let secret_bytes = Zeroizing::new(secret.to_bytes());

    // Deposit initiator's public key as presence in the handshake gate.
    // This creates the initiator's slot so GET(handshake, presence_slot)
    // works after the responder deposits their epk as the second slot.
    let commands = vec![ExchangeCommand::RelayEscrowDeposit {
        gate_hash: hex::decode(&handshake_slot).expect("hex from hex::encode is always valid"),
        slot_hash: hex::decode(&presence_slot).expect("hex from hex::encode is always valid"),
        encrypted_card: public.as_bytes().to_vec(),
        ttl_seconds: DEFAULT_TTL_SECONDS,
    }];

    let initiation = LinkInitiation {
        url,
        nonce,
        secret_key_bytes: secret_bytes,
        handshake_slot,
        presence_slot,
    };

    (initiation, commands)
}

/// Perform ECDH and derive escrow keys from the shared secret.
///
/// This is the first step after receiving the responder's public key.
/// The returned `EscrowKeys` contains the `card_key` needed to encrypt
/// the initiator's card before calling [`build_initiator_deposit`].
///
/// Returns `LinkModeError::NonContributoryDh` if the peer's public key is
/// a small-order point (attack mitigation — matches all other DH sites).
pub fn initiator_derive_keys(
    secret_key_bytes: &[u8; 32],
    peer_public_key: &[u8; 32],
) -> Result<EscrowKeys, LinkModeError> {
    let our_secret = StaticSecret::from(*secret_key_bytes);
    let their_public = PublicKey::from(*peer_public_key);
    let shared_secret = our_secret.diffie_hellman(&their_public);

    if !shared_secret.was_contributory() {
        return Err(LinkModeError::NonContributoryDh);
    }

    Ok(EscrowKeys::derive(
        shared_secret.as_bytes(),
        EscrowRole::Initiator,
    ))
}

/// Build the deposit command for the initiator's encrypted card.
///
/// Call after [`initiator_derive_keys`] and encrypting the card with
/// `EscrowKeys::encrypt_card`.
pub fn build_initiator_deposit(keys: &EscrowKeys, encrypted_card: Vec<u8>) -> Vec<ExchangeCommand> {
    vec![ExchangeCommand::RelayEscrowDeposit {
        gate_hash: hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"),
        slot_hash: hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid"),
        encrypted_card,
        ttl_seconds: DEFAULT_TTL_SECONDS,
    }]
}

/// Convenience: derive keys + build deposit in one call (for pre-encrypted cards).
///
/// Combines [`initiator_derive_keys`] + [`build_initiator_deposit`].
pub fn initiator_complete(
    secret_key_bytes: &[u8; 32],
    peer_public_key: &[u8; 32],
    encrypted_card: Vec<u8>,
) -> Result<(EscrowKeys, Vec<ExchangeCommand>), LinkModeError> {
    let keys = initiator_derive_keys(secret_key_bytes, peer_public_key)?;
    let commands = build_initiator_deposit(&keys, encrypted_card);
    Ok((keys, commands))
}

// =========================================================================
// Responder
// =========================================================================

/// Parsed Link URL components.
pub struct ParsedLinkUrl {
    /// Initiator's ephemeral X25519 public key (32 bytes).
    pub initiator_public_key: [u8; 32],
    /// Random nonce from the URL.
    pub nonce: [u8; 32],
}

/// Parse a link exchange URL.
///
/// Returns None if the URL format is invalid.
pub fn parse_link_url(url: &str) -> Option<ParsedLinkUrl> {
    let stripped = url.strip_prefix(LINK_URL_PREFIX)?;
    let query = stripped.strip_prefix('?')?;

    let mut pk_bytes = None;
    let mut nonce_bytes = None;

    for param in query.split('&') {
        if let Some((key, value)) = param.split_once('=') {
            match key {
                "pk" => {
                    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
                    if decoded.len() != 32 {
                        return None;
                    }
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&decoded);
                    pk_bytes = Some(arr);
                }
                "n" => {
                    let decoded = URL_SAFE_NO_PAD.decode(value).ok()?;
                    if decoded.len() != 32 {
                        return None;
                    }
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&decoded);
                    nonce_bytes = Some(arr);
                }
                _ => {}
            }
        }
    }

    Some(ParsedLinkUrl {
        initiator_public_key: pk_bytes?,
        nonce: nonce_bytes?,
    })
}

/// Commands for the responder after parsing the link URL.
///
/// Performs ECDH, derives escrow keys, and produces commands to:
/// 1. Deposit our ephemeral public key to the handshake slot
/// 2. Deposit our encrypted card to the escrow gate
///
/// Returns `LinkModeError::NonContributoryDh` if the initiator's public key
/// is a small-order point.
pub fn responder_respond(
    parsed: &ParsedLinkUrl,
    encrypted_card: Vec<u8>,
) -> Result<(EscrowKeys, Vec<ExchangeCommand>), LinkModeError> {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let our_public = PublicKey::from(&secret);
    let their_public = PublicKey::from(parsed.initiator_public_key);
    let shared_secret = secret.diffie_hellman(&their_public);

    if !shared_secret.was_contributory() {
        return Err(LinkModeError::NonContributoryDh);
    }

    let keys = EscrowKeys::derive(shared_secret.as_bytes(), EscrowRole::Responder);

    let handshake_slot = derive_handshake_slot(&parsed.nonce);
    let epk_slot = derive_epk_slot(&parsed.nonce);

    let commands = vec![
        // 1. Bootstrap: deposit our public key to handshake slot
        ExchangeCommand::RelayEscrowDeposit {
            gate_hash: hex::decode(&handshake_slot).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&epk_slot).expect("hex from hex::encode is always valid"),
            encrypted_card: our_public.as_bytes().to_vec(),
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
        // 2. Deposit encrypted card to escrow gate
        ExchangeCommand::RelayEscrowDeposit {
            gate_hash: hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid"),
            encrypted_card,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
    ];

    Ok((keys, commands))
}

// =========================================================================
// Internal helpers
// =========================================================================

/// Derive the handshake slot hash from the nonce.
///
/// `handshake_slot = H(nonce || "handshake")`
///
/// This is deliberately NOT derived from the shared secret, so the relay
/// cannot link the handshake slot to the escrow gate.
fn derive_handshake_slot(nonce: &[u8; 32]) -> String {
    let mut input = nonce.to_vec();
    input.extend_from_slice(b"handshake");
    hex::encode(Sha256::digest(&input))
}

/// Derive the slot hash for the ephemeral public key deposit.
///
/// `epk_slot = H(nonce || "epk")` — derived from the nonce so the relay
/// cannot fingerprint Link mode exchanges by a constant slot hash.
fn derive_epk_slot(nonce: &[u8; 32]) -> String {
    let mut input = nonce.to_vec();
    input.extend_from_slice(b"epk");
    hex::encode(Sha256::digest(&input))
}

/// Derive the initiator's presence slot in the handshake gate.
///
/// `presence_slot = H(nonce || "initiator_presence")`
///
/// The initiator deposits their public key to this slot before sharing
/// the URL. This ensures the handshake gate has 2 slots (presence + epk)
/// so the relay's GET authentication works correctly.
fn derive_initiator_presence_slot(nonce: &[u8; 32]) -> String {
    let mut input = nonce.to_vec();
    input.extend_from_slice(b"initiator_presence");
    hex::encode(Sha256::digest(&input))
}
