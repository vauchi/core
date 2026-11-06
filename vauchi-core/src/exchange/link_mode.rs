// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Link mode: asynchronous exchange via URL + relay escrow.
//!
//! The initiator generates a URL containing an ephemeral X25519 public key
//! and a random nonce, and shows a code to send another way (#727, SC-74). The
//! responder opens the URL and types the code; each side deposits its key and
//! SPAKE2 message, and derives [`EscrowKeys`] from the ECDH secret and the PAKE
//! key, so without the code the two sides sit in different gates.
//!
//! Core produces [`Command`]s — frontends execute relay calls and
//! report results via `Event`s (ADR-031).

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{EphemeralSecret, PublicKey, SharedSecret, StaticSecret};
use zeroize::Zeroizing;

use crate::card::ContactCard;
use crate::crypto::encryption::SymmetricKey;
use crate::crypto::kdf::HKDF;
use crate::crypto::signing::{Signature, SigningKeyPair, verify_signature};
use crate::crypto::x3dh::X3DHKeyPair;
use crate::exchange::escrow::{EscrowKeys, EscrowRole};
use crate::exchange::link_pake::{LINK_PAKE_MESSAGE_LEN, LinkCode, LinkPake, LinkPakeError};
use crate::platform::Command;

/// Payload version for the symmetric, updatable exchange (ADR-050): the
/// depositor's identity key, card, fresh X3DH exchange key and relay
/// routing, signed by the identity key for this exchange — so both sides
/// derive the same `shared_key` and establish a live update channel, not a
/// frozen import. v2 signed only the exchange key and relay URL, so a
/// rewritten card or a payload from another exchange still verified (#942).
///
/// Format: `[3][signature: 64][json body]`.
const CARD_PAYLOAD_VERSION: u8 = 3;

/// Domain-separation prefix for the link-mode bootstrap signature
/// (ADR-002/007). Signed over `[domain][nonce][initiator_epk]
/// [responder_epk][json body]`: the body is the only variable field and
/// comes last, so the layout is unambiguous without length prefixes, and
/// signing its exact bytes needs no canonical re-serialisation.
const LINK_BOOTSTRAP_DOMAIN: &[u8] = b"vauchi-link-bootstrap-v3";

const SIGNATURE_LEN: usize = 64;

/// HKDF info string for the persistent link-mode shared communication key
/// (ADR-007 domain separation).
const LINK_SHARED_KEY_INFO: &[u8] = b"vauchi-link-shared-key-v1";

/// Default TTL for escrow deposits (7 days, matching protocol max).
const DEFAULT_TTL_SECONDS: u32 = 604_800;

/// Exchange link a person without the app can open (vauchi/private#277).
/// Everything the exchange needs travels in the fragment, which browsers
/// never send to the server, so vauchi.app learns nothing about it.
const LINK_URL_PREFIX: &str = "https://vauchi.app/x#";

/// The form the landing page's Open button hands to an installed app.
const APP_LINK_URL_PREFIX: &str = "vauchi://exchange?";

/// The only link version a client takes. A link without it has no code behind
/// it, and accepting one would keep the attack the code removes (#727).
const LINK_VERSION: &str = "2";

/// An ephemeral X25519 key and a SPAKE2 message: the deposit each side makes in
/// the link's handshake gate.
const HANDSHAKE_BLOB_LEN: usize = 32 + LINK_PAKE_MESSAGE_LEN;

/// Error from Link mode operations.
#[derive(Debug, thiserror::Error)]
pub enum LinkModeError {
    /// The peer's public key is a small-order point (non-contributory DH).
    #[error("non-contributory Diffie-Hellman output (small-order point)")]
    NonContributoryDh,
    /// The peer's handshake deposit has the wrong length.
    #[error("malformed handshake deposit: expected {HANDSHAKE_BLOB_LEN} bytes, got {received}")]
    MalformedHandshakeBlob { received: usize },
    /// The sender's deposit names another key than the link does.
    #[error("the sender's deposit does not match the link")]
    PresenceKeyMismatch,
    /// The peer's SPAKE2 message was refused.
    #[error(transparent)]
    Pake(#[from] LinkPakeError),
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
    /// The ephemeral public key the URL carries.
    pub public_key: [u8; 32],
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
    /// The code to send to the responder another way than the link.
    pub code: LinkCode,
    /// Our side of the PAKE run, taken when the responder's message arrives.
    pub pake: Option<LinkPake>,
}

/// Generate a Link mode URL, ephemeral keypair, and presence deposit command.
///
/// Returns the initiation data and a command to deposit the initiator's
/// presence to the handshake gate. The presence deposit must be executed
/// before sharing the URL — it creates the initiator's slot in the
/// handshake gate so that GET works after the responder deposits their epk.
///
/// The caller must persist `secret_key_bytes` and `nonce` for later
/// use in [`initiator_derive_keys`].
pub fn initiator_generate() -> (LinkInitiation, Vec<Command>) {
    initiator_generate_named("")
}

/// Longest sender name a link carries, in bytes: keeps the link and its QR
/// short.
const LINK_NAME_MAX_BYTES: usize = 64;

/// Like [`initiator_generate`], with the sender's display name in the
/// fragment (`nm=`, base64url) so the vauchi.app/x landing page can say who
/// is sharing (vauchi/private#277). An empty name adds nothing.
pub fn initiator_generate_named(display_name: &str) -> (LinkInitiation, Vec<Command>) {
    // Use StaticSecret (not EphemeralSecret) because we need to persist
    // the secret key bytes for later ECDH. StaticSecret supports to_bytes().
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    let nonce: [u8; 32] = crate::crypto::random_bytes();

    let pk_b64 = URL_SAFE_NO_PAD.encode(public.as_bytes());
    let nonce_b64 = URL_SAFE_NO_PAD.encode(nonce);
    let mut url = format!("{LINK_URL_PREFIX}pk={pk_b64}&n={nonce_b64}&v={LINK_VERSION}");
    let name = truncate_to_char_boundary(display_name, LINK_NAME_MAX_BYTES);
    if !name.is_empty() {
        url.push_str("&nm=");
        url.push_str(&URL_SAFE_NO_PAD.encode(name));
    }

    let handshake_slot = derive_handshake_slot(&nonce);
    let presence_slot = derive_initiator_presence_slot(&nonce);
    let secret_bytes = Zeroizing::new(secret.to_bytes());
    let code = LinkCode::generate();
    let (pake, pake_message) = LinkPake::start(&code, &nonce, public.as_bytes());

    // Deposit initiator's public key as presence in the handshake gate.
    // This creates the initiator's slot so GET(handshake, presence_slot)
    // works after the responder deposits their epk as the second slot.
    let commands = vec![Command::RelayEscrowDeposit {
        gate_hash: hex::decode(&handshake_slot).expect("hex from hex::encode is always valid"),
        slot_hash: hex::decode(&presence_slot).expect("hex from hex::encode is always valid"),
        encrypted_card: handshake_blob(public.as_bytes(), &pake_message),
        ttl_seconds: DEFAULT_TTL_SECONDS,
    }];

    let initiation = LinkInitiation {
        url,
        public_key: *public.as_bytes(),
        nonce,
        secret_key_bytes: secret_bytes,
        handshake_slot,
        presence_slot,
        code,
        pake: Some(pake),
    };

    (initiation, commands)
}

fn handshake_blob(public_key: &[u8; 32], pake_message: &[u8]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(HANDSHAKE_BLOB_LEN);
    blob.extend_from_slice(public_key);
    blob.extend_from_slice(pake_message);
    blob
}

fn split_handshake_blob(blob: &[u8]) -> Result<([u8; 32], &[u8]), LinkModeError> {
    if blob.len() != HANDSHAKE_BLOB_LEN {
        return Err(LinkModeError::MalformedHandshakeBlob {
            received: blob.len(),
        });
    }
    let (public_key, pake_message) = blob.split_at(32);
    let mut key = [0u8; 32];
    key.copy_from_slice(public_key);
    Ok((key, pake_message))
}

/// Takes the responder's handshake deposit and derives the escrow keys from the
/// ECDH secret and the PAKE key. Returns the responder's epk with them.
///
/// A wrong code is not an error: it yields keys the responder does not hold.
///
/// # Errors
///
/// - [`LinkModeError::MalformedHandshakeBlob`] if the deposit has the wrong
///   length.
/// - [`LinkModeError::NonContributoryDh`] if the responder's key is a
///   small-order point (attack mitigation; matches all other DH sites).
/// - [`LinkModeError::Pake`] if the SPAKE2 message is invalid or our own.
pub fn initiator_derive_keys(
    secret_key_bytes: &[u8; 32],
    pake: LinkPake,
    responder_blob: &[u8],
) -> Result<([u8; 32], EscrowKeys), LinkModeError> {
    let (responder_epk, pake_message) = split_handshake_blob(responder_blob)?;
    let our_secret = StaticSecret::from(*secret_key_bytes);
    let shared_secret = our_secret.diffie_hellman(&PublicKey::from(responder_epk));

    if !shared_secret.was_contributory() {
        return Err(LinkModeError::NonContributoryDh);
    }

    let pake_key = pake.finish(pake_message)?;
    Ok((
        responder_epk,
        EscrowKeys::derive_with_pake(shared_secret.as_bytes(), &pake_key, EscrowRole::Initiator),
    ))
}

/// Build the deposit command for the initiator's encrypted card.
///
/// Call after [`initiator_derive_keys`] and encrypting the card with
/// `EscrowKeys::encrypt_card`.
pub fn build_escrow_deposit(keys: &EscrowKeys, encrypted_card: Vec<u8>) -> Vec<Command> {
    vec![Command::RelayEscrowDeposit {
        gate_hash: hex::decode(&keys.gate_hash).expect("hex from hex::encode is always valid"),
        slot_hash: hex::decode(&keys.our_slot).expect("hex from hex::encode is always valid"),
        encrypted_card,
        ttl_seconds: DEFAULT_TTL_SECONDS,
    }]
}

/// Parsed Link URL components.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParsedLinkUrl {
    /// Initiator's ephemeral X25519 public key (32 bytes).
    pub initiator_public_key: [u8; 32],
    /// Random nonce from the URL.
    pub nonce: [u8; 32],
    /// The name the link says its sender has (`nm=`). Unchecked: anyone
    /// can put any name in a link, so it is shown only as a claim.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_name: Option<String>,
}

/// A sender name from a link, or `None` when it cannot be shown as a plain
/// name. A dropped name never invalidates the link.
fn decode_sender_name(value: &str) -> Option<String> {
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    if bytes.len() > LINK_NAME_MAX_BYTES {
        return None;
    }
    let name = String::from_utf8(bytes).ok()?;
    let name = name.trim();
    if name.is_empty()
        || name
            .chars()
            .any(|c| c.is_control() || hides_or_reorders_text(c))
    {
        return None;
    }
    Some(name.to_owned())
}

/// Invisible and text-direction characters that make a displayed name read
/// differently from what it is (`Zoë\u{202E}knab` shows as "Zoëbank").
fn hides_or_reorders_text(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

fn truncate_to_char_boundary(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Parse a link exchange URL.
///
/// Returns None if the URL format is invalid or it is not a [`LINK_VERSION`]
/// link.
pub fn parse_link_url(url: &str) -> Option<ParsedLinkUrl> {
    let query = url
        .strip_prefix(LINK_URL_PREFIX)
        .or_else(|| url.strip_prefix(APP_LINK_URL_PREFIX))?;

    let mut supported_version = false;
    let mut pk_bytes = None;
    let mut nonce_bytes = None;
    let mut sender_name = None;

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
                "nm" => sender_name = decode_sender_name(value),
                "v" => supported_version = value == LINK_VERSION,
                _ => {}
            }
        }
    }

    if !supported_version {
        return None;
    }

    Some(ParsedLinkUrl {
        initiator_public_key: pk_bytes?,
        nonce: nonce_bytes?,
        sender_name,
    })
}

// =========================================================================
// Deep-link consent gate parser (problem record
// 2026-04-25-deeplink-consent-orchestrator)
// =========================================================================

/// Parsed payload from a deep-link URI handed in by the OS.
///
/// Wraps [`ParsedLinkUrl`]; held by the consent screen until the user
/// grants or denies. A grant leads to the code screen, and the typed code to
/// [`responder_open`].
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

    /// The sender name the link claims; unchecked, see
    /// [`ParsedLinkUrl::sender_name`].
    pub fn sender_name(&self) -> Option<&str> {
        self.inner.sender_name.as_deref()
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
    /// `vauchi://exchange?pk=<b64>&n=<b64>&v=2`. Distinguished from
    /// `MalformedQuery` so a future frontend can show "this link uses
    /// an old format — ask the sender for a fresh one" if useful.
    #[error("legacy path form")]
    LegacyPathForm,
    /// Query parameters are missing, malformed (bad base64), have
    /// wrong-length keys, or name no supported link version.
    #[error("malformed query")]
    MalformedQuery,
}

/// Parse a `vauchi://exchange?pk=<b64>&n=<b64>&v=2` URI into a
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

    // 0. The https form (#277): only vauchi.app/x with a fragment payload.
    if let Some(rest) = uri.strip_prefix("https://") {
        if !rest.starts_with("vauchi.app/x#") {
            return Err(DeepLinkParseError::InvalidHost);
        }
        let parsed = parse_link_url(uri).ok_or(DeepLinkParseError::MalformedQuery)?;
        return Ok(DeepLinkPayload { inner: parsed });
    }

    // 1. Otherwise the scheme must be `vauchi`.
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
    //    generator emits `vauchi://exchange?pk=...&n=...&v=2`.
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

/// The responder's side of a link, from the typed code until the sender's
/// deposit gives the PAKE key.
///
/// The responder deposits before it holds the escrow keys, because the relay
/// serves the sender's deposit only once the gate holds two slots.
pub struct LinkOpening {
    shared_secret: SharedSecret,
    pake: LinkPake,
    public_key: [u8; 32],
    pake_message: Vec<u8>,
    binding: LinkExchangeBinding,
}

impl LinkOpening {
    /// The exchange both bootstraps are signed for.
    pub fn binding(&self) -> &LinkExchangeBinding {
        &self.binding
    }

    /// The link's handshake gate.
    pub fn handshake_gate(&self) -> Vec<u8> {
        handshake_gate(&self.binding.nonce)
    }

    /// Our slot in the handshake gate: the one the relay writes once, which is
    /// what limits guessing to one per link.
    pub fn epk_slot(&self) -> Vec<u8> {
        hex::decode(derive_epk_slot(&self.binding.nonce))
            .expect("hex from hex::encode is always valid")
    }

    /// Our deposit in the handshake gate: `pk_r || msg_r`.
    pub fn deposit_command(&self) -> Command {
        Command::RelayEscrowDeposit {
            gate_hash: self.handshake_gate(),
            slot_hash: self.epk_slot(),
            encrypted_card: handshake_blob(&self.public_key, &self.pake_message),
            ttl_seconds: DEFAULT_TTL_SECONDS,
        }
    }

    /// Takes the sender's handshake deposit and derives the escrow keys.
    ///
    /// A wrong code is not an error: it yields keys the sender does not hold.
    ///
    /// # Errors
    ///
    /// - [`LinkModeError::MalformedHandshakeBlob`] if the deposit has the wrong
    ///   length.
    /// - [`LinkModeError::PresenceKeyMismatch`] if it names another key than
    ///   the link.
    /// - [`LinkModeError::Pake`] if the SPAKE2 message is invalid or our own.
    pub fn finish(self, presence_blob: &[u8]) -> Result<EscrowKeys, LinkModeError> {
        let (initiator_epk, pake_message) = split_handshake_blob(presence_blob)?;
        if initiator_epk != self.binding.initiator_epk {
            return Err(LinkModeError::PresenceKeyMismatch);
        }
        let pake_key = self.pake.finish(pake_message)?;
        Ok(EscrowKeys::derive_with_pake(
            self.shared_secret.as_bytes(),
            &pake_key,
            EscrowRole::Responder,
        ))
    }
}

/// Responder side of a link, once the person has typed the code: ECDH with a
/// fresh ephemeral key and the start of the PAKE run. Nothing has reached the
/// relay yet; the deposit is [`LinkOpening::deposit_command`].
///
/// # Errors
///
/// [`LinkModeError::NonContributoryDh`] if the initiator's public key is a
/// small-order point.
pub fn responder_open(
    parsed: &ParsedLinkUrl,
    code: &LinkCode,
) -> Result<LinkOpening, LinkModeError> {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let our_public = PublicKey::from(&secret);
    let shared_secret = secret.diffie_hellman(&PublicKey::from(parsed.initiator_public_key));
    if !shared_secret.was_contributory() {
        return Err(LinkModeError::NonContributoryDh);
    }
    let (pake, pake_message) = LinkPake::start(code, &parsed.nonce, &parsed.initiator_public_key);
    Ok(LinkOpening {
        shared_secret,
        pake,
        public_key: *our_public.as_bytes(),
        pake_message,
        binding: LinkExchangeBinding {
            nonce: parsed.nonce,
            initiator_epk: parsed.initiator_public_key,
            responder_epk: *our_public.as_bytes(),
        },
    })
}

/// Decrypt the initiator's encrypted card retrieved from escrow.
///
/// After the responder polls the relay and the initiator's deposit lands at
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

/// The exchange a link bootstrap is signed for: the link's nonce and the
/// two ephemeral escrow keys. A payload signed for one exchange fails in any
/// other, and when a link's key is swapped in transit the two sides hold
/// different bindings, so neither accepts the other's relayed card (#942).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkExchangeBinding {
    /// The link's nonce.
    pub nonce: [u8; 32],
    /// The ephemeral key the link carries.
    pub initiator_epk: [u8; 32],
    /// The ephemeral key the responder deposited in the handshake gate.
    pub responder_epk: [u8; 32],
}

/// A parsed link-mode card payload: the symmetric-exchange bootstrap
/// (ADR-050), carrying the peer's X3DH exchange key and relay routing for a
/// live update channel. Its identity signature is verified on parse.
#[derive(Debug, Clone)]
pub struct LinkCardPayload {
    /// The peer's identity signing key.
    pub identity_pubkey: [u8; 32],
    /// The peer's X3DH exchange key.
    pub x3dh_pubkey: [u8; 32],
    /// The peer's relay.
    pub relay_url: String,
    /// The peer's card.
    pub card: ContactCard,
}

/// Serde body of a payload (the bytes after the signature).
#[derive(Serialize, Deserialize)]
struct CardPayloadBody {
    identity_pubkey: [u8; 32],
    x3dh_pubkey: [u8; 32],
    relay_url: String,
    card: ContactCard,
}

/// Build the domain-separated message the bootstrap signature covers.
fn bootstrap_signing_message(binding: &LinkExchangeBinding, body: &[u8]) -> Vec<u8> {
    [
        LINK_BOOTSTRAP_DOMAIN,
        &binding.nonce,
        &binding.initiator_epk,
        &binding.responder_epk,
        body,
    ]
    .concat()
}

/// Serialize a link-mode card payload (ADR-050): identity key + card, plus
/// the depositor's fresh X3DH exchange key and relay routing, signed by the
/// identity key for `binding` so the peer can verify the bootstrap.
///
/// Format: `[3][signature: 64][json of CardPayloadBody]`.
pub fn serialize_link_card_payload(
    signing_keypair: &SigningKeyPair,
    x3dh_pubkey: &[u8; 32],
    relay_url: &str,
    card: &ContactCard,
    binding: &LinkExchangeBinding,
) -> Vec<u8> {
    let body = CardPayloadBody {
        identity_pubkey: *signing_keypair.public_key().as_bytes(),
        x3dh_pubkey: *x3dh_pubkey,
        relay_url: relay_url.to_string(),
        card: card.clone(),
    };
    let body = serde_json::to_vec(&body).expect("card payload serialization should not fail");
    let signature = signing_keypair.sign(&bootstrap_signing_message(binding, &body));
    [&[CARD_PAYLOAD_VERSION][..], signature.as_bytes(), &body].concat()
}

/// Parse a link-mode card payload signed for `binding`. Only the current
/// version is accepted, and its signature is verified against the embedded
/// identity key; any failure is a rejection (fail-closed).
pub fn parse_link_card_payload(
    data: &[u8],
    binding: &LinkExchangeBinding,
) -> Result<LinkCardPayload, LinkModeError> {
    let rest = match data.split_first() {
        Some((&CARD_PAYLOAD_VERSION, rest)) => rest,
        Some((other, _)) => {
            return Err(LinkModeError::MalformedCardPayload(format!(
                "unsupported version byte: {other}"
            )));
        }
        None => {
            return Err(LinkModeError::MalformedCardPayload(
                "empty payload".to_string(),
            ));
        }
    };
    let Some((signature, body_bytes)) = rest.split_first_chunk::<SIGNATURE_LEN>() else {
        return Err(LinkModeError::MalformedCardPayload(format!(
            "payload shorter than its {SIGNATURE_LEN}-byte signature"
        )));
    };
    let body: CardPayloadBody = serde_json::from_slice(body_bytes)
        .map_err(|e| LinkModeError::MalformedCardPayload(e.to_string()))?;
    if !verify_signature(
        &body.identity_pubkey,
        &bootstrap_signing_message(binding, body_bytes),
        &Signature::from_bytes(*signature),
    ) {
        return Err(LinkModeError::MalformedCardPayload(
            "bootstrap signature verification failed".to_string(),
        ));
    }
    Ok(LinkCardPayload {
        identity_pubkey: body.identity_pubkey,
        x3dh_pubkey: body.x3dh_pubkey,
        relay_url: body.relay_url,
        card: body.card,
    })
}

/// Derive the persistent shared communication key for a link-mode exchange
/// (ADR-050) from our fresh X3DH keypair and the peer's X3DH public key
/// (carried in the bootstrap). A single authenticated DH —
/// *authenticated* because the peer's X3DH key is signed by their identity
/// in the payload, *forward-secure* because both keys are per-exchange.
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

/// The link's handshake gate: where the initiator's presence and the
/// responder's epk are deposited.
pub fn handshake_gate(nonce: &[u8; 32]) -> Vec<u8> {
    hex::decode(derive_handshake_slot(nonce)).expect("hex from hex::encode is always valid")
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
