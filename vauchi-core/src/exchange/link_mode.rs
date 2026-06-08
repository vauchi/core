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
//! Core produces [`Command`]s — frontends execute relay calls and
//! report results via `Event`s (ADR-031).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::rngs::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::contact_card::ContactCard;
use crate::crypto::encryption::SymmetricKey;
use crate::crypto::kdf::HKDF;
use crate::crypto::signing::{Signature, SigningKeyPair, verify_signature};
use crate::crypto::x3dh::X3DHKeyPair;
use crate::exchange::escrow::{EscrowKeys, EscrowRole};
use crate::platform::Command;

/// Version byte prefixing a serialized link-mode card payload.
const CARD_PAYLOAD_VERSION: u8 = 1;

/// Payload version for the symmetric, updatable exchange (ADR-050). In
/// addition to the v1 `[identity_pubkey][card]`, a v2 payload carries the
/// depositor's fresh X3DH exchange public key, its relay routing, and an
/// identity signature over that bootstrap — so both sides derive the same
/// `shared_key` and establish a live update channel, not a frozen import.
const CARD_PAYLOAD_VERSION_V2: u8 = 2;

/// Domain-separation prefix for the link-mode bootstrap signature
/// (ADR-002/007). Signed over `[domain][x3dh_pubkey][relay_noise(32, zeros
/// if none)][relay_url]` — fixed-length fields first so the variable
/// `relay_url` tail is unambiguous.
const LINK_BOOTSTRAP_DOMAIN: &[u8] = b"vauchi-link-bootstrap-v2";

/// HKDF info string for the persistent link-mode shared communication key
/// (ADR-007 domain separation).
const LINK_SHARED_KEY_INFO: &[u8] = b"vauchi-link-shared-key-v1";

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
    /// A decrypted card payload could not be parsed (too short, wrong
    /// version byte, or invalid card JSON).
    #[error("malformed card payload: {0}")]
    MalformedCardPayload(String),
}

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
pub fn initiator_generate() -> (LinkInitiation, Vec<Command>) {
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
    let commands = vec![Command::RelayEscrowDeposit {
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
pub fn build_initiator_deposit(keys: &EscrowKeys, encrypted_card: Vec<u8>) -> Vec<Command> {
    vec![Command::RelayEscrowDeposit {
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
) -> Result<(EscrowKeys, Vec<Command>), LinkModeError> {
    let keys = initiator_derive_keys(secret_key_bytes, peer_public_key)?;
    let commands = build_initiator_deposit(&keys, encrypted_card);
    Ok((keys, commands))
}

/// Parsed Link URL components.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

// =========================================================================
// Deep-link consent gate parser (problem record
// 2026-04-25-deeplink-consent-orchestrator)
// =========================================================================

/// Parsed payload from a deep-link URI handed in by the OS.
///
/// Wraps [`ParsedLinkUrl`]; held by the consent screen until the user
/// grants or denies, at which point the gate's grant action drives
/// [`responder_respond`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeepLinkPayload {
    inner: ParsedLinkUrl,
}

impl DeepLinkPayload {
    /// Initiator's ephemeral X25519 public key (32 bytes).
    pub fn initiator_public_key(&self) -> &[u8; 32] {
        &self.inner.initiator_public_key
    }

    /// Random nonce from the URL.
    pub fn nonce(&self) -> &[u8; 32] {
        &self.inner.nonce
    }

    /// Borrow the wrapped [`ParsedLinkUrl`] so the responder flow can
    /// drive the existing link-mode functions without re-parsing.
    pub fn as_parsed(&self) -> &ParsedLinkUrl {
        &self.inner
    }
}

/// Reason an external deep-link URI was rejected.
///
/// Variants are distinct so the consent layer can surface a typed
/// rejection reason in the toast / error banner that replaces the
/// dropped `DeepLinkHandler.swift` / `DeepLinkHandler.kt` invalid
/// branch.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeepLinkParseError {
    /// URI scheme is not `vauchi`.
    #[error("invalid scheme")]
    InvalidScheme,
    /// URI host is not `exchange`.
    #[error("invalid host")]
    InvalidHost,
    /// URI uses the legacy `vauchi://exchange/<payload>` path-component
    /// form. This shape was a placeholder in the original frontend
    /// handlers; it never lit up in production and the live core
    /// link-mode generator emits the query form
    /// `vauchi://exchange?pk=<b64>&n=<b64>`. Distinguished from
    /// `MalformedQuery` so a future frontend can show "this link uses
    /// an old format — ask the sender for a fresh one" if useful.
    #[error("legacy path form")]
    LegacyPathForm,
    /// Query parameters are missing, malformed (bad base64), or have
    /// wrong-length keys.
    #[error("malformed query")]
    MalformedQuery,
}

/// Parse a `vauchi://exchange?pk=<b64>&n=<b64>` URI into a
/// [`DeepLinkPayload`] suitable for handing to the consent screen.
///
/// Returns a typed [`DeepLinkParseError`] for every rejection reason;
/// the consent layer maps these to user-visible messages. The path
/// component form `vauchi://exchange/<payload>` (a defunct placeholder
/// from the original frontend handlers) is rejected with
/// [`DeepLinkParseError::LegacyPathForm`].
pub fn parse_exchange_deep_link(uri: &str) -> Result<DeepLinkPayload, DeepLinkParseError> {
    // Walk the URI manually rather than pulling in a URL crate — the
    // accepted shape is fully constrained by `LINK_URL_PREFIX`, and
    // every divergence corresponds to one typed error variant.

    // 1. Scheme must be `vauchi`.
    let after_scheme = uri
        .strip_prefix("vauchi://")
        .ok_or(DeepLinkParseError::InvalidScheme)?;

    // 2. Host (up to next `?` or `/` or end) must be `exchange`.
    let host_end = after_scheme.find(['?', '/']).unwrap_or(after_scheme.len());
    let host = &after_scheme[..host_end];
    if host != "exchange" {
        return Err(DeepLinkParseError::InvalidHost);
    }

    // 3. Distinguish path-form from query-form. The legacy frontend
    //    handlers parsed `vauchi://exchange/<payload>`; the live core
    //    generator emits `vauchi://exchange?pk=...&n=...`.
    let after_host = &after_scheme[host_end..];
    if after_host.starts_with('/') {
        return Err(DeepLinkParseError::LegacyPathForm);
    }

    // 4. Query form: delegate to the existing parser, mapping its
    //    `Option::None` to `MalformedQuery`. This covers missing
    //    params, bad base64, and wrong-length keys — every failure
    //    `parse_link_url` bails on.
    let parsed = parse_link_url(uri).ok_or(DeepLinkParseError::MalformedQuery)?;
    Ok(DeepLinkPayload { inner: parsed })
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
) -> Result<(EscrowKeys, Vec<Command>), LinkModeError> {
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
        Command::RelayEscrowDeposit {
            gate_hash: hex::decode(&handshake_slot).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&epk_slot).expect("hex from hex::encode is always valid"),
            encrypted_card: our_public.as_bytes().to_vec(),
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
        // 2. Deposit encrypted card to escrow gate
        Command::RelayEscrowDeposit {
            gate_hash: hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid"),
            encrypted_card,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
    ];

    Ok((keys, commands))
}

/// Responder-side single-shot helper: ECDH, derive keys, encrypt the
/// caller-supplied raw card bytes with the derived `card_key`, and
/// produce the same two deposit commands as
/// [`responder_respond`].
///
/// This is the production-ergonomic counterpart to [`responder_respond`],
/// which takes pre-encrypted bytes — but the encryption depends on
/// keys [`responder_respond`] derives internally, leaving callers with a
/// chicken-and-egg problem. This helper closes the gap so frontends
/// can pass their raw card serialization without exposing
/// [`EscrowKeys::encrypt_card`] through the UniFFI surface.
///
/// # Errors
///
/// - [`LinkModeError::NonContributoryDh`] if the initiator's public
///   key is a small-order point.
/// - [`LinkModeError::CardCryptoFailed`] if encryption fails (e.g.,
///   the AEAD nonce RNG fails — extremely rare but typed for the
///   cycle thread to surface as `on_failed(DecryptError)`).
pub fn responder_respond_with_card_bytes(
    parsed: &ParsedLinkUrl,
    raw_card_bytes: &[u8],
) -> Result<(EscrowKeys, Vec<Command>), LinkModeError> {
    // ECDH + key derivation, identical to `responder_respond`. We
    // duplicate rather than refactor so neither path's tests have to
    // change shape — `responder_respond` stays a stable opaque-bytes
    // entry point for tests and future direct callers, while this
    // helper layers the encrypt step on top.
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let our_public = PublicKey::from(&secret);
    let their_public = PublicKey::from(parsed.initiator_public_key);
    let shared_secret = secret.diffie_hellman(&their_public);
    if !shared_secret.was_contributory() {
        return Err(LinkModeError::NonContributoryDh);
    }
    let keys = EscrowKeys::derive(shared_secret.as_bytes(), EscrowRole::Responder);

    let encrypted = keys
        .encrypt_card(raw_card_bytes)
        .map_err(|e| LinkModeError::CardCryptoFailed(e.to_string()))?;

    let handshake_slot = derive_handshake_slot(&parsed.nonce);
    let epk_slot = derive_epk_slot(&parsed.nonce);

    let commands = vec![
        Command::RelayEscrowDeposit {
            gate_hash: hex::decode(&handshake_slot).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&epk_slot).expect("hex from hex::encode is always valid"),
            encrypted_card: our_public.as_bytes().to_vec(),
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
        Command::RelayEscrowDeposit {
            gate_hash: hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"),
            slot_hash: hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid"),
            encrypted_card: encrypted,
            ttl_seconds: DEFAULT_TTL_SECONDS,
        },
    ];

    Ok((keys, commands))
}

/// Decrypt the initiator's encrypted card retrieved from escrow.
///
/// Symmetric counterpart to [`responder_respond`]: after the responder
/// polls the relay and the initiator's deposit lands at
/// `(keys.gate_hash, keys.their_slot)`, this function turns the
/// retrieved blob into plaintext bytes ready for parsing as a contact
/// card payload.
///
/// # Errors
///
/// Returns [`LinkModeError::CardCryptoFailed`] for any decrypt failure
/// (truncated nonce, AEAD authentication failure, wrong key). The cycle
/// thread maps this to `on_failed(DecryptError)` so the frontend can
/// render a typed toast instead of a silent dismissal.
///
/// # Why a thin wrapper
///
/// `EscrowKeys::decrypt_card` already does the work; this wrapper
/// exists to (a) name the operation in domain terms ("responder
/// completes by decrypting") and (b) map the crypto-layer error onto
/// the link-mode error vocabulary that the cycle thread already speaks.
pub fn responder_complete(
    keys: &EscrowKeys,
    encrypted_card_blob: &[u8],
) -> Result<Vec<u8>, LinkModeError> {
    keys.decrypt_card(encrypted_card_blob)
        .map_err(|e| LinkModeError::CardCryptoFailed(e.to_string()))
}

/// Serialize an identity signing key + contact card into the link-mode
/// card payload both sides swap over escrow.
///
/// Format: `[version: 1 byte][public_key: 32 bytes][card_json: rest]`.
/// This is the plaintext fed to [`responder_respond_with_card_bytes`]
/// (which encrypts it) and recovered by [`parse_card_payload`] after
/// [`responder_complete`] decrypts the peer's deposit.
pub fn serialize_card_payload(public_key: &[u8; 32], card: &ContactCard) -> Vec<u8> {
    let card_json = serde_json::to_vec(card).expect("ContactCard serialization should not fail");
    let mut payload = Vec::with_capacity(1 + 32 + card_json.len());
    payload.push(CARD_PAYLOAD_VERSION);
    payload.extend_from_slice(public_key);
    payload.extend_from_slice(&card_json);
    payload
}

/// Parse a link-mode card payload into `(signing_public_key, card)`.
///
/// Inverse of [`serialize_card_payload`]. Rejects payloads shorter than
/// the 33-byte header, an unrecognized version byte, or invalid card
/// JSON with [`LinkModeError::MalformedCardPayload`].
pub fn parse_card_payload(data: &[u8]) -> Result<([u8; 32], ContactCard), LinkModeError> {
    if data.len() < 33 {
        return Err(LinkModeError::MalformedCardPayload(format!(
            "payload too short: {} bytes",
            data.len()
        )));
    }
    if data[0] != CARD_PAYLOAD_VERSION {
        return Err(LinkModeError::MalformedCardPayload(format!(
            "unsupported version byte: {}",
            data[0]
        )));
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&data[1..33]);
    let card: ContactCard = serde_json::from_slice(&data[33..])
        .map_err(|e| LinkModeError::MalformedCardPayload(e.to_string()))?;
    Ok((public_key, card))
}

/// A parsed link-mode card payload — either the legacy v1
/// (`[identity_pubkey][card]`, which yields an *import*) or the v2
/// symmetric-exchange bootstrap (ADR-050), which additionally carries the
/// peer's X3DH exchange key + relay routing for establishing a live update
/// channel. [`parse_card_payload_versioned`] dispatches on the version byte
/// and, for v2, verifies the identity signature before returning.
#[derive(Debug, Clone)]
pub enum LinkCardPayload {
    /// Legacy import payload — no update channel.
    V1 {
        identity_pubkey: [u8; 32],
        card: ContactCard,
    },
    /// Symmetric exchange bootstrap — signature already verified.
    V2 {
        identity_pubkey: [u8; 32],
        x3dh_pubkey: [u8; 32],
        relay_url: String,
        relay_noise_pubkey: Option<[u8; 32]>,
        card: ContactCard,
    },
}

/// Serde body of a v2 payload (the bytes after the version byte).
#[derive(Serialize, Deserialize)]
struct CardPayloadV2Body {
    identity_pubkey: [u8; 32],
    x3dh_pubkey: [u8; 32],
    relay_url: String,
    #[serde(default)]
    relay_noise_pubkey: Option<[u8; 32]>,
    signature: Vec<u8>,
    card: ContactCard,
}

/// Build the domain-separated message the v2 bootstrap signature covers.
fn bootstrap_signing_message(
    x3dh_pubkey: &[u8; 32],
    relay_url: &str,
    relay_noise_pubkey: &Option<[u8; 32]>,
) -> Vec<u8> {
    let mut message = Vec::with_capacity(LINK_BOOTSTRAP_DOMAIN.len() + 32 + 32 + relay_url.len());
    message.extend_from_slice(LINK_BOOTSTRAP_DOMAIN);
    message.extend_from_slice(x3dh_pubkey);
    message.extend_from_slice(&relay_noise_pubkey.unwrap_or([0u8; 32]));
    message.extend_from_slice(relay_url.as_bytes());
    message
}

/// Serialize a v2 link-mode card payload (ADR-050): identity key + card,
/// plus the depositor's fresh X3DH exchange key and relay routing, signed
/// by the identity key so the peer can verify the bootstrap.
///
/// Format: `[version: 2][json of CardPayloadV2Body]`.
pub fn serialize_card_payload_v2(
    identity_pubkey: &[u8; 32],
    signing_keypair: &SigningKeyPair,
    x3dh_pubkey: &[u8; 32],
    relay_url: &str,
    relay_noise_pubkey: Option<[u8; 32]>,
    card: &ContactCard,
) -> Vec<u8> {
    let message = bootstrap_signing_message(x3dh_pubkey, relay_url, &relay_noise_pubkey);
    let signature = signing_keypair.sign(&message);
    let body = CardPayloadV2Body {
        identity_pubkey: *identity_pubkey,
        x3dh_pubkey: *x3dh_pubkey,
        relay_url: relay_url.to_string(),
        relay_noise_pubkey,
        signature: signature.as_bytes().to_vec(),
        card: card.clone(),
    };
    let json = serde_json::to_vec(&body).expect("v2 card payload serialization should not fail");
    let mut payload = Vec::with_capacity(1 + json.len());
    payload.push(CARD_PAYLOAD_VERSION_V2);
    payload.extend_from_slice(&json);
    payload
}

/// Parse a link-mode card payload of either version, dispatching on the
/// leading version byte. For v2 the bootstrap signature is verified against
/// the embedded identity key; a bad signature is rejected (fail-closed).
pub fn parse_card_payload_versioned(data: &[u8]) -> Result<LinkCardPayload, LinkModeError> {
    match data.first() {
        Some(&CARD_PAYLOAD_VERSION) => {
            let (identity_pubkey, card) = parse_card_payload(data)?;
            Ok(LinkCardPayload::V1 {
                identity_pubkey,
                card,
            })
        }
        Some(&CARD_PAYLOAD_VERSION_V2) => {
            let body: CardPayloadV2Body = serde_json::from_slice(&data[1..])
                .map_err(|e| LinkModeError::MalformedCardPayload(e.to_string()))?;
            let sig_bytes: [u8; 64] = body.signature.as_slice().try_into().map_err(|_| {
                LinkModeError::MalformedCardPayload(format!(
                    "bootstrap signature must be 64 bytes, got {}",
                    body.signature.len()
                ))
            })?;
            let message = bootstrap_signing_message(
                &body.x3dh_pubkey,
                &body.relay_url,
                &body.relay_noise_pubkey,
            );
            if !verify_signature(
                &body.identity_pubkey,
                &message,
                &Signature::from_bytes(sig_bytes),
            ) {
                return Err(LinkModeError::MalformedCardPayload(
                    "bootstrap signature verification failed".to_string(),
                ));
            }
            Ok(LinkCardPayload::V2 {
                identity_pubkey: body.identity_pubkey,
                x3dh_pubkey: body.x3dh_pubkey,
                relay_url: body.relay_url,
                relay_noise_pubkey: body.relay_noise_pubkey,
                card: body.card,
            })
        }
        Some(&other) => Err(LinkModeError::MalformedCardPayload(format!(
            "unsupported version byte: {other}"
        ))),
        None => Err(LinkModeError::MalformedCardPayload(
            "empty payload".to_string(),
        )),
    }
}

/// Derive the persistent shared communication key for a link-mode exchange
/// (ADR-050) from our fresh X3DH keypair and the peer's X3DH public key
/// (carried in the v2 bootstrap). A single authenticated DH —
/// *authenticated* because the peer's X3DH key is signed by their identity
/// in the v2 payload, *forward-secure* because both keys are per-exchange.
/// Both sides derive the **same** key (X25519 DH is commutative), so the
/// resulting `ExchangedData.shared_key` is symmetric.
pub fn derive_link_shared_key(
    our_x3dh: &X3DHKeyPair,
    their_x3dh_public: &[u8; 32],
) -> Result<SymmetricKey, LinkModeError> {
    let shared = our_x3dh
        .diffie_hellman(their_x3dh_public)
        .map_err(|e| LinkModeError::CardCryptoFailed(e.to_string()))?;
    let derived = HKDF::derive_key(None, &shared[..], LINK_SHARED_KEY_INFO);
    Ok(SymmetricKey::from_bytes(*derived))
}

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
