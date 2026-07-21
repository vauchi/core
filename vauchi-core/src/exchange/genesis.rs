// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Genesis envelope — registry-free first-contact session bootstrap (ADR-068).
//!
//! A device that holds a contact's `shared_key` but has no established session
//! (a secondary device that joined by device-link and never did the QR
//! exchange) must still be able to send a signed safety alert. It seals the
//! alert into a **wire-ordinary [`RatchetMessage`]** whose Double Ratchet is
//! rooted directly in `shared_key`, carrying the sender's device announcement
//! and full signed registry broadcast inside the ciphertext.
//!
//! ## Security properties (ADR-068 §Security, MR B scope)
//!
//! - **No forward secrecy on genesis msg#1.** The responder's DH half is
//!   derived from `shared_key` alone, so a later `shared_key` compromise
//!   retroactively decrypts the genesis message. FS resumes at the receiver's
//!   first reply (the first real DH ratchet step). Ratified — do not
//!   re-litigate.
//! - **Fresh single-use header key (F1).** The header `dh_public` is the
//!   initiator ratchet's own ephemeral, never the static device exchange key.
//! - **`shared_key` is admission to the parser, never authority (ADR req 4).**
//!   The envelope carries an identity signature that binds both identities,
//!   the epoch, the exact outer ratchet header (F7), the device announcement,
//!   and the exact inner alert bytes. Nothing is trusted before it verifies.
//! - **Stateless open (F8).** The receiver derives its responder state from
//!   `shared_key` + the message header on each attempt and never persists a
//!   transient row; a bounded chain index caps key-derivation work.
//!
//! MR B keeps the receiver's post-accept session under the legacy `[0;32]`
//! ratchet id (today's production HTTP receive path routes everything there);
//! canonical per-device sessions and the msg#2 acknowledgement belong to the
//! later routing program. See
//! `planning/todo/2026-07-21-genesis-envelope-plan.md` §REVISION.

use crate::crypto::x3dh::X3DHKeyPair;
use crate::crypto::{DoubleRatchetState, HKDF, RatchetMessage, SymmetricKey};
use crate::identity::{Identity, RegistryBroadcast};

/// Version byte of the genesis envelope inner payload. Distinct from the
/// [`crate::sync::delta::VersionedPayload`] tags (`0x02`–`0x04`) — a genesis
/// envelope wraps one of those as its inner payload.
pub const PAYLOAD_VERSION_GENESIS: u8 = 0x05;

/// Domain separator for the envelope identity signature.
const GENESIS_ENVELOPE_DOMAIN: &[u8] = b"vauchi-sync-genesis-envelope-v1";

/// HKDF `info` prefix for the genesis session root. `shared_key` is the IKM;
/// the directional identity pair follows so the two directions never collide.
const GENESIS_ROOT_INFO: &[u8] = b"vauchi-genesis-root-v1";

/// HKDF `info` prefix for the deterministic genesis responder keypair — the
/// receiver-side DH half both parties can derive from `shared_key`.
const GENESIS_RESPONDER_INFO: &[u8] = b"vauchi-genesis-responder-v1";

/// Maximum genesis chain index the receiver will derive to. Legitimate genesis
/// is index 0; a higher claimed index is rejected before any key derivation so
/// a `shared_key` holder cannot force skipped-key inflation (F8/F7).
pub const GENESIS_MAX_CHAIN_INDEX: u32 = 64;

/// Maximum genesis envelope plaintext, kept below the top padding bucket
/// (4096 − the 4-byte length prefix) so a registry-bearing genesis never
/// pushes into an overflow bucket that would leak its size class (F10).
pub const MAX_GENESIS_PLAINTEXT: usize = 4092;

/// Errors from sealing or opening a genesis envelope.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GenesisError {
    /// Ratchet initialization, encryption, or decryption failed. On the
    /// receive side this is the "not a genesis message under this shared_key"
    /// signal — fail closed.
    #[error("genesis crypto failure: {0}")]
    Crypto(String),

    /// The decoded envelope is structurally malformed (bad version byte,
    /// truncated, or trailing bytes).
    #[error("malformed genesis envelope")]
    InvalidFormat,

    /// The envelope identity signature did not verify against the claimed
    /// sender identity, both identities, the epoch, the exact outer header,
    /// the device announcement, and the exact inner alert bytes.
    #[error("genesis envelope signature invalid")]
    SignatureInvalid,

    /// The message's chain index exceeds [`GENESIS_MAX_CHAIN_INDEX`].
    #[error("genesis chain index out of bounds")]
    ChainIndexTooHigh,

    /// The envelope plaintext exceeds [`MAX_GENESIS_PLAINTEXT`].
    #[error("genesis envelope too large")]
    TooLarge,
}

/// A verified, opened genesis envelope plus the advanced receiver ratchet.
///
/// The caller must still verify the inner alert's own signature and run its
/// replay/idempotency checks before any user-visible effect — possession of
/// `shared_key` and a valid envelope signature admit the alert to the alert
/// parser, they do not authorize it.
#[derive(Debug)]
pub struct OpenedGenesis {
    /// Announced sending device id (from inside the ciphertext).
    pub sender_device_id: [u8; 32],
    /// Announced sending device exchange public key.
    pub sender_exchange_public_key: [u8; 32],
    /// Sender-stamped routing epoch (diagnostic; not a freshness gate — F6).
    pub epoch: u64,
    /// The sender's full signed registry broadcast, as raw JSON, for a
    /// best-effort additive topology merge by the caller (F2/F3).
    pub sender_registry_broadcast_json: Vec<u8>,
    /// The exact inner [`crate::sync::delta::VersionedPayload`] bytes (a signed
    /// alert, `0x04`) the caller routes through its normal alert handling.
    pub inner_payload: Vec<u8>,
    /// The receiver ratchet advanced past this message — persist as the
    /// `[0;32]` session so subsequent ordinary messages decrypt normally.
    pub advanced_ratchet: DoubleRatchetState,
}

/// The genesis envelope builder/parser. Stateless; all methods are associated
/// functions operating on `shared_key`, identities, and a [`RatchetMessage`].
pub struct GenesisEnvelope;

impl GenesisEnvelope {
    /// Seal `inner_payload` (a `VersionedPayload`-encoded signed alert) into a
    /// wire-ordinary [`RatchetMessage`] rooted in `shared_key`.
    ///
    /// Returns the message to queue and the advanced initiator ratchet to
    /// persist under the `[0;32]` session id for this contact.
    pub fn seal(
        shared_key: &SymmetricKey,
        sender: &Identity,
        recipient_identity: &[u8; 32],
        sender_broadcast: &RegistryBroadcast,
        epoch: u64,
        inner_payload: &[u8],
    ) -> Result<(RatchetMessage, DoubleRatchetState), GenesisError> {
        let sender_identity = sender.signing_public_key();
        let root = genesis_root(shared_key, sender_identity, recipient_identity);
        let responder_public =
            *genesis_responder_keypair(shared_key, sender_identity, recipient_identity)
                .public_key();

        let mut session = DoubleRatchetState::initialize_initiator(&root, responder_public)
            .map_err(|e| GenesisError::Crypto(format!("init initiator: {e:?}")))?;

        let sender_device_id = *sender.device_id();
        let sender_exchange_public_key = *sender.device_info().exchange_public_key();
        let broadcast_json = serde_json::to_vec(sender_broadcast)
            .map_err(|e| GenesisError::Crypto(format!("broadcast encode: {e}")))?;

        // Genesis msg#1 header is deterministic (`initialize_initiator` starts
        // at generation 0, index 0, previous-chain 0); the signature binds the
        // exact fields `encrypt` will emit so a shared_key holder cannot
        // re-wrap the ciphertext under a different header (F7).
        let header = MessageHeader {
            dh_public: session.our_public_key(),
            dh_generation: 0,
            message_index: 0,
            previous_chain_length: 0,
        };
        let signable = signable_bytes(
            sender_identity,
            recipient_identity,
            epoch,
            &header,
            &sender_device_id,
            &sender_exchange_public_key,
            &broadcast_json,
            inner_payload,
        );
        let signature = *sender.sign(&signable).as_bytes();

        let plaintext = encode_envelope(
            epoch,
            &sender_device_id,
            &sender_exchange_public_key,
            &signature,
            &broadcast_json,
            inner_payload,
        );
        if plaintext.len() > MAX_GENESIS_PLAINTEXT {
            return Err(GenesisError::TooLarge);
        }

        let message = session
            .encrypt(&plaintext)
            .map_err(|e| GenesisError::Crypto(format!("encrypt: {e:?}")))?;
        Ok((message, session))
    }

    /// Open a candidate genesis [`RatchetMessage`] statelessly from
    /// `shared_key` + the message header, verifying the envelope signature
    /// against both identities and the exact received header.
    ///
    /// `sender_identity` is the alert sender (the genesis initiator);
    /// `recipient_identity` is the local identity. Fails closed on any error;
    /// the caller treats a failure as "not a genesis message" and retains the
    /// underlying decrypt error.
    pub fn open(
        shared_key: &SymmetricKey,
        sender_identity: &[u8; 32],
        recipient_identity: &[u8; 32],
        message: &RatchetMessage,
    ) -> Result<OpenedGenesis, GenesisError> {
        if message.message_index > GENESIS_MAX_CHAIN_INDEX {
            return Err(GenesisError::ChainIndexTooHigh);
        }
        let root = genesis_root(shared_key, sender_identity, recipient_identity);
        let responder_keypair =
            genesis_responder_keypair(shared_key, sender_identity, recipient_identity);

        let mut session = DoubleRatchetState::initialize_responder(&root, responder_keypair);
        let plaintext = session
            .decrypt(message)
            .map_err(|e| GenesisError::Crypto(format!("decrypt: {e:?}")))?;

        let decoded = decode_envelope(&plaintext)?;

        let header = MessageHeader {
            dh_public: message.dh_public,
            dh_generation: message.dh_generation,
            message_index: message.message_index,
            previous_chain_length: message.previous_chain_length,
        };
        let signable = signable_bytes(
            sender_identity,
            recipient_identity,
            decoded.epoch,
            &header,
            &decoded.sender_device_id,
            &decoded.sender_exchange_public_key,
            &decoded.broadcast_json,
            &decoded.inner_payload,
        );
        let sender_pk = crate::crypto::PublicKey::from_bytes(*sender_identity);
        if !sender_pk.verify(
            &signable,
            &crate::crypto::Signature::from_bytes(decoded.signature),
        ) {
            return Err(GenesisError::SignatureInvalid);
        }

        Ok(OpenedGenesis {
            sender_device_id: decoded.sender_device_id,
            sender_exchange_public_key: decoded.sender_exchange_public_key,
            epoch: decoded.epoch,
            sender_registry_broadcast_json: decoded.broadcast_json,
            inner_payload: decoded.inner_payload,
            advanced_ratchet: session,
        })
    }
}

/// The outer ratchet header fields bound into the envelope signature.
struct MessageHeader {
    dh_public: [u8; 32],
    dh_generation: u32,
    message_index: u32,
    previous_chain_length: u32,
}

/// Genesis session root key: `HKDF(ikm = shared_key, info = domain ||
/// sender_identity || recipient_identity)`. Directional — the identity order
/// keeps the two directions' roots distinct (F1: no `sender_device_id`, which
/// is only available post-decrypt).
fn genesis_root(
    shared_key: &SymmetricKey,
    sender_identity: &[u8; 32],
    recipient_identity: &[u8; 32],
) -> SymmetricKey {
    let mut info = Vec::with_capacity(GENESIS_ROOT_INFO.len() + 64);
    info.extend_from_slice(GENESIS_ROOT_INFO);
    info.extend_from_slice(sender_identity);
    info.extend_from_slice(recipient_identity);
    SymmetricKey::from_bytes(*HKDF::derive_key(None, shared_key.as_bytes(), &info))
}

/// The deterministic genesis responder keypair both parties derive from
/// `shared_key`. The sender uses only its public half (as the initiator's peer
/// DH key); the receiver holds the private half to open the message.
fn genesis_responder_keypair(
    shared_key: &SymmetricKey,
    sender_identity: &[u8; 32],
    recipient_identity: &[u8; 32],
) -> X3DHKeyPair {
    let mut info = Vec::with_capacity(GENESIS_RESPONDER_INFO.len() + 64);
    info.extend_from_slice(GENESIS_RESPONDER_INFO);
    info.extend_from_slice(sender_identity);
    info.extend_from_slice(recipient_identity);
    X3DHKeyPair::from_bytes(*HKDF::derive_key(None, shared_key.as_bytes(), &info))
}

#[allow(clippy::too_many_arguments)]
fn signable_bytes(
    sender_identity: &[u8; 32],
    recipient_identity: &[u8; 32],
    epoch: u64,
    header: &MessageHeader,
    sender_device_id: &[u8; 32],
    sender_exchange_public_key: &[u8; 32],
    broadcast_json: &[u8],
    inner_payload: &[u8],
) -> Vec<u8> {
    let mut msg = Vec::with_capacity(
        GENESIS_ENVELOPE_DOMAIN.len()
            + 32
            + 32
            + 8
            + 44
            + 32
            + 32
            + broadcast_json.len()
            + inner_payload.len()
            + 8,
    );
    msg.extend_from_slice(GENESIS_ENVELOPE_DOMAIN);
    msg.extend_from_slice(sender_identity);
    msg.extend_from_slice(recipient_identity);
    msg.extend_from_slice(&epoch.to_be_bytes());
    msg.extend_from_slice(&header.dh_public);
    msg.extend_from_slice(&header.dh_generation.to_be_bytes());
    msg.extend_from_slice(&header.message_index.to_be_bytes());
    msg.extend_from_slice(&header.previous_chain_length.to_be_bytes());
    msg.extend_from_slice(sender_device_id);
    msg.extend_from_slice(sender_exchange_public_key);
    msg.extend_from_slice(&(broadcast_json.len() as u32).to_be_bytes());
    msg.extend_from_slice(broadcast_json);
    msg.extend_from_slice(&(inner_payload.len() as u32).to_be_bytes());
    msg.extend_from_slice(inner_payload);
    msg
}

/// Envelope wire layout (inner plaintext, before ratchet encryption):
/// `0x05 || epoch(8) || device_id(32) || exchange_pub(32) || signature(64)
///  || broadcast_len(4) || broadcast_json || alert_len(4) || alert_bytes`.
fn encode_envelope(
    epoch: u64,
    sender_device_id: &[u8; 32],
    sender_exchange_public_key: &[u8; 32],
    signature: &[u8; 64],
    broadcast_json: &[u8],
    inner_payload: &[u8],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(
        1 + 8 + 32 + 32 + 64 + 4 + broadcast_json.len() + 4 + inner_payload.len(),
    );
    buf.push(PAYLOAD_VERSION_GENESIS);
    buf.extend_from_slice(&epoch.to_be_bytes());
    buf.extend_from_slice(sender_device_id);
    buf.extend_from_slice(sender_exchange_public_key);
    buf.extend_from_slice(signature);
    buf.extend_from_slice(&(broadcast_json.len() as u32).to_be_bytes());
    buf.extend_from_slice(broadcast_json);
    buf.extend_from_slice(&(inner_payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(inner_payload);
    buf
}

struct DecodedEnvelope {
    epoch: u64,
    sender_device_id: [u8; 32],
    sender_exchange_public_key: [u8; 32],
    signature: [u8; 64],
    broadcast_json: Vec<u8>,
    inner_payload: Vec<u8>,
}

/// Bounded decode: every length prefix must fit the remaining buffer and the
/// buffer must be exactly consumed (no trailing bytes). Peer-supplied lengths
/// never drive an allocation past the remaining input.
fn decode_envelope(data: &[u8]) -> Result<DecodedEnvelope, GenesisError> {
    if data.len() > MAX_GENESIS_PLAINTEXT {
        return Err(GenesisError::TooLarge);
    }
    let mut cursor = Reader::new(data);
    if cursor.u8()? != PAYLOAD_VERSION_GENESIS {
        return Err(GenesisError::InvalidFormat);
    }
    let epoch = cursor.u64()?;
    let sender_device_id = cursor.array32()?;
    let sender_exchange_public_key = cursor.array32()?;
    let signature = cursor.array64()?;
    let broadcast_len = cursor.u32()? as usize;
    let broadcast_json = cursor.take(broadcast_len)?.to_vec();
    let inner_len = cursor.u32()? as usize;
    let inner_payload = cursor.take(inner_len)?.to_vec();
    if !cursor.is_empty() {
        return Err(GenesisError::InvalidFormat);
    }
    Ok(DecodedEnvelope {
        epoch,
        sender_device_id,
        sender_exchange_public_key,
        signature,
        broadcast_json,
        inner_payload,
    })
}

/// Minimal bounds-checked byte reader — every accessor fails closed on
/// truncation rather than panicking on a slice out of range.
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], GenesisError> {
        let end = self.pos.checked_add(n).ok_or(GenesisError::InvalidFormat)?;
        if end > self.data.len() {
            return Err(GenesisError::InvalidFormat);
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, GenesisError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, GenesisError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, GenesisError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }

    fn array32(&mut self) -> Result<[u8; 32], GenesisError> {
        let b = self.take(32)?;
        let mut a = [0u8; 32];
        a.copy_from_slice(b);
        Ok(a)
    }

    fn array64(&mut self) -> Result<[u8; 64], GenesisError> {
        let b = self.take(64)?;
        let mut a = [0u8; 64];
        a.copy_from_slice(b);
        Ok(a)
    }

    fn is_empty(&self) -> bool {
        self.pos == self.data.len()
    }
}

// INLINE_TEST_REQUIRED: exercises the private wire-format decoder
// (encode_envelope/decode_envelope/Reader) at its parse boundary (DC-01);
// truncation/oversize/trailing-byte cases cannot be reached through the public
// seal/open API, which only produces well-formed envelopes.
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_envelope() -> Vec<u8> {
        encode_envelope(
            42,
            &[1u8; 32],
            &[2u8; 32],
            &[3u8; 64],
            b"broadcast-json",
            b"\x04inner-alert",
        )
    }

    // @internal
    #[test]
    fn decode_roundtrips_a_well_formed_envelope() {
        let decoded = decode_envelope(&sample_envelope()).expect("well-formed envelope decodes");
        assert_eq!(decoded.epoch, 42);
        assert_eq!(decoded.sender_device_id, [1u8; 32]);
        assert_eq!(decoded.sender_exchange_public_key, [2u8; 32]);
        assert_eq!(decoded.signature, [3u8; 64]);
        assert_eq!(decoded.broadcast_json, b"broadcast-json");
        assert_eq!(decoded.inner_payload, b"\x04inner-alert");
    }

    // @internal
    #[test]
    fn decode_rejects_trailing_bytes() {
        let mut bytes = sample_envelope();
        bytes.push(0xff);
        assert!(matches!(
            decode_envelope(&bytes),
            Err(GenesisError::InvalidFormat)
        ));
    }

    // @internal
    #[test]
    fn decode_rejects_truncation_at_every_prefix_boundary() {
        let full = sample_envelope();
        // Every proper prefix is truncated and must fail closed, never panic.
        for len in 0..full.len() {
            assert!(
                matches!(
                    decode_envelope(&full[..len]),
                    Err(GenesisError::InvalidFormat)
                ),
                "truncation to {len} bytes must be rejected"
            );
        }
    }

    // @internal
    #[test]
    fn decode_rejects_wrong_version_byte() {
        let mut bytes = sample_envelope();
        bytes[0] = 0x04;
        assert!(matches!(
            decode_envelope(&bytes),
            Err(GenesisError::InvalidFormat)
        ));
    }

    // @internal
    #[test]
    fn decode_rejects_oversized_input() {
        let oversized = vec![PAYLOAD_VERSION_GENESIS; MAX_GENESIS_PLAINTEXT + 1];
        assert!(matches!(
            decode_envelope(&oversized),
            Err(GenesisError::TooLarge)
        ));
    }

    // @internal
    #[test]
    fn decode_rejects_a_length_prefix_beyond_the_buffer() {
        // A broadcast_len that claims more bytes than remain must not allocate
        // or panic — it fails closed (DC-01).
        let mut bytes = Vec::new();
        bytes.push(PAYLOAD_VERSION_GENESIS);
        bytes.extend_from_slice(&7u64.to_be_bytes());
        bytes.extend_from_slice(&[1u8; 32]);
        bytes.extend_from_slice(&[2u8; 32]);
        bytes.extend_from_slice(&[3u8; 64]);
        bytes.extend_from_slice(&u32::MAX.to_be_bytes()); // absurd broadcast_len
        bytes.extend_from_slice(b"only-a-few-bytes");
        assert!(matches!(
            decode_envelope(&bytes),
            Err(GenesisError::InvalidFormat)
        ));
    }

    // @internal
    #[test]
    fn open_rejects_a_message_that_decrypts_but_has_a_bad_envelope_signature() {
        // Distinct from the identity-swap black-box test (which fails at the
        // decrypt step because the identities feed the root KDF): here the root
        // matches, so decrypt SUCCEEDS, and only the envelope signature check
        // (F7) can reject the message — the defense-in-depth layer on top of
        // the header AEAD. Exercises the SignatureInvalid path directly.
        use crate::identity::Identity;

        let alice = Identity::create("Alice", 0);
        let bob = Identity::create("Bob", 0);
        let shared = SymmetricKey::generate();
        let sender = alice.signing_public_key();
        let recipient = bob.signing_public_key();

        let root = genesis_root(&shared, sender, recipient);
        let responder_public = *genesis_responder_keypair(&shared, sender, recipient).public_key();
        let mut session = DoubleRatchetState::initialize_initiator(&root, responder_public)
            .expect("initiator init");

        // Encode an envelope carrying a deliberately-invalid signature, then
        // encrypt it with a correctly-rooted session so decrypt will succeed.
        let plaintext = encode_envelope(1, &[9u8; 32], &[8u8; 32], &[0u8; 64], b"bc", b"\x04inner");
        let message = session.encrypt(&plaintext).expect("encrypt");

        let err = GenesisEnvelope::open(&shared, sender, recipient, &message)
            .expect_err("a bad envelope signature must be rejected after a successful decrypt");
        assert!(
            matches!(err, GenesisError::SignatureInvalid),
            "expected SignatureInvalid, got {err:?}"
        );
    }
}
