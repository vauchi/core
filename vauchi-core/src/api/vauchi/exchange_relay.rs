// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Relay-mediated contact exchange — Vauchi API methods.
//!
//! Provides `start_relay_exchange`, `claim_relay_exchange`, and
//! `complete_relay_exchange` on [`Vauchi`]. These orchestrate the full
//! protocol: X3DH key agreement, contact creation, Double Ratchet
//! initialization, and SAS verification code derivation.
//!
//! ## Protocol flow
//!
//! 1. **Initiator** calls `start_relay_exchange()` → gets a short numeric `code`.
//! 2. **Responder** enters the code and calls `claim_relay_exchange(code)` →
//!    gets `RelayExchangeResult` with the new contact and a SAS code.
//! 3. **Initiator** polls `complete_relay_exchange(code, &mut offer)` until it
//!    returns `Some(RelayExchangeResult)` with a matching SAS code.
//! 4. Both sides verbally compare SAS codes to confirm the exchange.
//!
//! ## X3DH roles
//!
//! The **responder** (claim side) acts as the X3DH *initiator* — it generates
//! the ephemeral key. The **offer initiator** (complete side) acts as the X3DH
//! *responder*.
//!
//! ## Payload format
//!
//! Offer payload (base64-encoded JSON):
//! ```json
//! { "identity_key": "<b64>", "exchange_key": "<b64>", "display_name": "Alice" }
//! ```
//!
//! Response payload (base64-encoded JSON, includes ephemeral):
//! ```json
//! { "identity_key": "<b64>", "exchange_key": "<b64>",
//!   "ephemeral_key": "<b64>", "display_name": "Bob" }
//! ```

use base64::Engine;
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use super::Vauchi;
use crate::api::contact_manager::ContactManager;
use crate::api::error::{VauchiError, VauchiResult};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::exchange::relay_exchange::derive_sas;
use crate::exchange::{X3DH, X3DHKeyPair};
use crate::network::HttpTransport;

/// Result of `start_relay_exchange`: the code to display and secret
/// material needed to complete the exchange later.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct RelayExchangeOffer {
    /// The short numeric code the responder enters.
    pub code: String,
    /// Our X3DH keypair secret bytes — needed for X3DH::respond in
    /// `complete_relay_exchange`. Stored as raw bytes so this struct
    /// doesn't carry the full keypair across the API boundary.
    sas_key_material: [u8; 32],
    /// Our signing (identity) public key — needed for SAS derivation.
    our_identity_key: [u8; 32],
}

impl std::fmt::Debug for RelayExchangeOffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayExchangeOffer")
            .field("code", &"[REDACTED]")
            .field("sas_key_material", &"[REDACTED]")
            .field("our_identity_key", &self.our_identity_key)
            .finish()
    }
}

impl RelayExchangeOffer {
    /// Returns the X3DH key material for completing the exchange.
    ///
    /// Only exposed for testing — production callers pass the whole
    /// `RelayExchangeOffer` to `complete_relay_exchange`.
    #[cfg(any(test, feature = "testing"))]
    pub fn sas_key_material(&self) -> &[u8; 32] {
        &self.sas_key_material
    }

    /// Returns our identity key embedded in the offer.
    #[cfg(any(test, feature = "testing"))]
    pub fn our_identity_key(&self) -> &[u8; 32] {
        &self.our_identity_key
    }
}

/// Result of a completed relay exchange (claim or complete side).
#[derive(Debug)]
pub struct RelayExchangeResult {
    /// The newly created contact's ID (hex of their signing key).
    pub contact_id: String,
    /// The other party's display name.
    pub display_name: String,
    /// Short Authentication String for verbal comparison ("XXX-XXX").
    pub sas: String,
}

// ── Payload types for JSON serialization ──────────────────────────

/// Offer payload: what the initiator sends to the relay.
#[derive(Serialize, Deserialize)]
struct OfferPayload {
    identity_key: String,
    exchange_key: String,
    display_name: String,
}

/// Response payload: what the responder sends back (includes ephemeral).
#[derive(Serialize, Deserialize)]
struct ResponsePayload {
    identity_key: String,
    exchange_key: String,
    ephemeral_key: String,
    display_name: String,
}

// ── Helpers ───────────────────────────────────────────────────────

fn b64_encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn b64_decode_32(s: &str) -> VauchiResult<[u8; 32]> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(s)
        .map_err(|e| VauchiError::Serialization(format!("base64 decode: {e}")))?;
    bytes
        .try_into()
        .map_err(|_| VauchiError::Serialization("expected 32 bytes for key field".into()))
}

fn encode_payload_b64(json: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(json)
}

fn decode_payload_b64(b64: &str) -> VauchiResult<Vec<u8>> {
    base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|e| VauchiError::Serialization(format!("payload base64 decode: {e}")))
}

// ── Vauchi methods ────────────────────────────────────────────────

impl Vauchi {
    /// Starts a relay-mediated contact exchange as the initiator.
    ///
    /// Posts an offer containing our public keys and display name to the
    /// relay. Returns a [`RelayExchangeOffer`] with the short numeric code
    /// the responder must enter, plus secret material for later completion.
    ///
    /// The offer payload is plain JSON (not encrypted) — it contains only
    /// public keys and a display name, matching the QR exchange model.
    pub fn start_relay_exchange(
        &self,
        expires_secs: Option<u64>,
    ) -> VauchiResult<RelayExchangeOffer> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_x3dh = X3DHKeyPair::generate();
        let our_identity_key = *identity.signing_public_key();

        // Build offer payload
        let payload = OfferPayload {
            identity_key: b64_encode(&our_identity_key),
            exchange_key: b64_encode(our_x3dh.public_key()),
            display_name: identity.display_name().to_string(),
        };
        let payload_json = serde_json::to_vec(&payload)
            .map_err(|e| VauchiError::Serialization(format!("offer payload: {e}")))?;
        let payload_b64 = encode_payload_b64(&payload_json);

        // Post to relay
        let transport = self.create_relay_transport();
        let code = transport
            .exchange_offer(&payload_b64, expires_secs)
            .map_err(VauchiError::Network)?;

        Ok(RelayExchangeOffer {
            code,
            sas_key_material: *our_x3dh.secret_bytes(),
            our_identity_key,
        })
    }

    /// Claims a relay exchange as the responder.
    ///
    /// Fetches the initiator's offer, performs X3DH key agreement (as the
    /// X3DH *initiator*), creates the contact, initializes the Double
    /// Ratchet, and derives a SAS verification code.
    ///
    /// The response payload includes an ephemeral key so the offer
    /// initiator can compute the matching shared secret.
    pub fn claim_relay_exchange(&self, code: &str) -> VauchiResult<RelayExchangeResult> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_x3dh = identity.x3dh_keypair();
        let our_identity_key = *identity.signing_public_key();

        // The claim API is a single round-trip: we send our response AND
        // receive the initiator's offer atomically. We pre-generate the
        // ephemeral key independently (it doesn't depend on the peer's
        // key), include it in our response, then compute the X3DH shared
        // secret after learning the initiator's exchange key.

        use crate::crypto::kdf::HKDF;
        use rand_core::OsRng;
        use x25519_dalek::{EphemeralSecret, PublicKey};
        use zeroize::Zeroize;

        // 1. Generate ephemeral for forward secrecy
        let ephemeral_secret = EphemeralSecret::random_from_rng(OsRng);
        let ephemeral_public = PublicKey::from(&ephemeral_secret);

        // 2. Build response payload with our keys + ephemeral
        let response = ResponsePayload {
            identity_key: b64_encode(&our_identity_key),
            exchange_key: b64_encode(our_x3dh.public_key()),
            ephemeral_key: b64_encode(ephemeral_public.as_bytes()),
            display_name: identity.display_name().to_string(),
        };
        let response_json = serde_json::to_vec(&response)
            .map_err(|e| VauchiError::Serialization(format!("response payload: {e}")))?;
        let response_b64 = encode_payload_b64(&response_json);

        // 3. Claim the offer — send our response, get initiator's payload
        let transport = self.create_relay_transport();
        let offer_b64 = transport
            .exchange_claim(code, &response_b64)
            .map_err(VauchiError::Network)?;

        // 4. Decode the initiator's offer payload
        let offer_json = decode_payload_b64(&offer_b64)?;
        let offer: OfferPayload = serde_json::from_slice(&offer_json)
            .map_err(|e| VauchiError::Serialization(format!("offer decode: {e}")))?;
        let their_identity_key = b64_decode_32(&offer.identity_key)?;
        let their_exchange_key = b64_decode_32(&offer.exchange_key)?;

        // 5. Check contact doesn't already exist
        let public_id = hex::encode(their_identity_key);
        if self.storage.contacts().load_contact(&public_id)?.is_some() {
            return Err(VauchiError::Configuration(format!(
                "Contact {public_id} already exists"
            )));
        }

        // 6. Compute X3DH shared secret (we are X3DH initiator)
        //    DH1: our_static × their_static (identity binding)
        //    DH2: ephemeral × their_static (forward secrecy)
        let their_static = PublicKey::from(their_exchange_key);
        let dh1 = our_x3dh.diffie_hellman(&their_exchange_key).map_err(|_| {
            VauchiError::Exchange(crate::exchange::ExchangeError::KeyAgreementFailed(
                "DH1 failed: non-contributory output".into(),
            ))
        })?;
        let dh2_shared = ephemeral_secret.diffie_hellman(&their_static);
        if !dh2_shared.was_contributory() {
            return Err(VauchiError::Exchange(
                crate::exchange::ExchangeError::KeyAgreementFailed(
                    "DH2 failed: non-contributory output".into(),
                ),
            ));
        }
        let dh2 = *dh2_shared.as_bytes();

        let mut ikm = [0u8; 64];
        ikm[..32].copy_from_slice(&*dh1);
        ikm[32..].copy_from_slice(&dh2);
        let derived = HKDF::derive_key(None, &ikm, b"vauchi-x3dh-key-v2");
        ikm.zeroize();
        let shared_secret = crate::crypto::SymmetricKey::from_bytes(*derived);

        // 7. Create contact
        let card = ContactCard::new(&offer.display_name);
        let contact = Contact::from_exchange(
            their_identity_key,
            card,
            shared_secret.clone(),
            self.clock.unix_seconds(),
        );
        let contact_id = contact.id().to_string();

        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)?;

        // 8. Initialize Double Ratchet as X3DH initiator
        self.create_ratchet_as_initiator(&contact_id, &shared_secret, their_exchange_key)?;

        // 9. Derive SAS
        let sas = derive_sas(
            shared_secret.as_bytes(),
            &our_identity_key,
            &their_identity_key,
        );

        Ok(RelayExchangeResult {
            contact_id,
            display_name: offer.display_name,
            sas,
        })
    }

    /// Completes a relay exchange as the initiator (poll for response).
    ///
    /// Returns `Ok(None)` if the responder hasn't claimed yet.
    /// Returns `Ok(Some(result))` once the responder has claimed and the
    /// contact + ratchet have been created.
    pub fn complete_relay_exchange(
        &self,
        code: &str,
        offer: &mut RelayExchangeOffer,
    ) -> VauchiResult<Option<RelayExchangeResult>> {
        let _identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        // Poll the relay
        let transport = self.create_relay_transport();
        let response_b64 = match transport
            .exchange_complete(code)
            .map_err(VauchiError::Network)?
        {
            Some(b64) => b64,
            None => return Ok(None),
        };

        // Decode the responder's payload
        let response_json = decode_payload_b64(&response_b64)?;
        let response: ResponsePayload = serde_json::from_slice(&response_json)
            .map_err(|e| VauchiError::Serialization(format!("response decode: {e}")))?;
        let their_identity_key = b64_decode_32(&response.identity_key)?;
        let their_exchange_key = b64_decode_32(&response.exchange_key)?;
        let their_ephemeral_key = b64_decode_32(&response.ephemeral_key)?;

        // Check contact doesn't already exist
        let public_id = hex::encode(their_identity_key);
        if self.storage.contacts().load_contact(&public_id)?.is_some() {
            return Err(VauchiError::Configuration(format!(
                "Contact {public_id} already exists"
            )));
        }

        // X3DH as responder: we are the "displayer" in X3DH terms.
        // DH1: our_static × their_static (their_exchange_key is identity for DH1)
        // DH2: our_static × their_ephemeral
        let our_x3dh = X3DHKeyPair::from_bytes(offer.sas_key_material);
        let shared_secret = X3DH::respond(&our_x3dh, &their_exchange_key, &their_ephemeral_key)
            .map_err(|e| {
                VauchiError::Exchange(crate::exchange::ExchangeError::KeyAgreementFailed(format!(
                    "X3DH respond failed: {e:?}"
                )))
            })?;

        // Create contact
        let card = ContactCard::new(&response.display_name);
        let contact = Contact::from_exchange(
            their_identity_key,
            card,
            shared_secret.clone(),
            self.clock.unix_seconds(),
        );
        let contact_id = contact.id().to_string();

        let manager = ContactManager::new(&self.storage, self.events.clone());
        manager.add_contact(contact)?;

        // Initialize Double Ratchet as X3DH responder
        let ratchet_dh = X3DHKeyPair::from_bytes(*our_x3dh.secret_bytes());
        self.create_ratchet_as_responder(&contact_id, &shared_secret, ratchet_dh)?;

        // Derive SAS
        let sas = derive_sas(
            shared_secret.as_bytes(),
            &offer.our_identity_key,
            &their_identity_key,
        );

        // The relay offer is single-use. Destroy its private key and claim
        // capability immediately after successful protocol completion.
        offer.zeroize();

        Ok(Some(RelayExchangeResult {
            contact_id,
            display_name: response.display_name,
            sas,
        }))
    }

    /// Create an `HttpTransport` for relay-mediated exchange operations.
    ///
    /// Exchange payloads are non-secret (they contain only public keys and
    /// display names — the same data shared in a QR exchange), but the
    /// caller's source IP is still sensitive: a relay that sees two IPs
    /// coordinating an exchange learns who met whom. ADR-037 therefore
    /// requires OHTTP here as well.
    fn create_relay_transport(&self) -> HttpTransport {
        self.build_relay_transport(
            &self.config.relay.server_url,
            self.config.relay.connect_timeout_ms,
        )
    }
}
