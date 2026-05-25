// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact exchange operations for mobile.

use std::sync::Arc;

use vauchi_core::contact_card::ContactCard;

use super::VauchiPlatform;
use super::error::MobileError;
use super::exchange::MobileExchangeSession;
use super::multistage_exchange::MobileMultiStageSession;
use super::types::MobileExchangeResult;

/// Exchange payload format version byte.
const EXCHANGE_PAYLOAD_VERSION: u8 = 1;

/// Serialize identity public key + contact card into an exchange payload.
///
/// Format: `[version: 1 byte][public_key: 32 bytes][card_json: rest]`
pub(crate) fn serialize_exchange_payload(public_key: &[u8; 32], card: &ContactCard) -> Vec<u8> {
    let card_json = serde_json::to_vec(card).expect("ContactCard serialization should not fail");
    let mut payload = Vec::with_capacity(1 + 32 + card_json.len());
    payload.push(EXCHANGE_PAYLOAD_VERSION);
    payload.extend_from_slice(public_key);
    payload.extend_from_slice(&card_json);
    payload
}

/// Deserialize an exchange payload into (public_key, ContactCard).
pub(crate) fn deserialize_exchange_payload(
    data: &[u8],
) -> Result<([u8; 32], ContactCard), MobileError> {
    if data.len() < 34 {
        return Err(MobileError::Other {
            detail: "Exchange payload too short".to_string(),
        });
    }
    let version = data[0];
    if version != EXCHANGE_PAYLOAD_VERSION {
        return Err(MobileError::Other {
            detail: format!("Unsupported exchange payload version: {}", version),
        });
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&data[1..33]);
    let card: ContactCard =
        serde_json::from_slice(&data[33..]).map_err(|e| MobileError::Other {
            detail: format!("Failed to deserialize contact card: {}", e),
        })?;
    Ok((public_key, card))
}

#[uniffi::export]
impl VauchiPlatform {
    // === Exchange Operations ===

    /// Create a QR exchange session with manual confirmation (no audio hardware).
    pub fn create_qr_exchange_manual(&self) -> Result<Arc<MobileExchangeSession>, MobileError> {
        let identity = self.get_identity()?;
        let our_card = self.get_own_card_or_default(&identity)?;
        Ok(super::exchange::create_qr_exchange_manual(
            identity, our_card,
        ))
    }

    /// Finalize a completed exchange session.
    ///
    /// Extracts the contact from the session's Complete state, saves it to storage,
    /// and initializes the double ratchet. No relay notification is sent — face-to-face
    /// exchange completes locally on both devices.
    ///
    /// On repeat exchange with the same peer, the contact card is upserted and the
    /// ratchet state is re-initialized with fresh keys. Any relay messages in flight
    /// from the previous ratchet epoch become permanently undecryptable. This is
    /// intentional: a face-to-face re-exchange is a deliberate key ceremony that
    /// establishes fresh forward secrecy.
    ///
    /// Old mailbox tokens (derived from the previous shared key) self-heal
    /// via the relay's 30-day blob TTL — no active deregistration needed.
    /// `exchange_timestamp` is updated by the SQL upsert in `save_contact()`.
    ///
    /// The session must be in the Complete state (i.e., the state machine has been
    /// driven through all steps).
    pub fn finalize_exchange(
        &self,
        session: &MobileExchangeSession,
    ) -> Result<MobileExchangeResult, MobileError> {
        let contact = session.extract_contact()?;
        let storage = self.open_storage()?;

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        // Upsert: save_contact uses INSERT ON CONFLICT UPDATE, so
        // repeated exchanges with the same peer update the card data
        // rather than failing.
        storage.save_contact(&contact)?;

        // Initialize the Double Ratchet via the session seam, which selects the
        // role (initiator/responder) deterministically and keys off the X25519
        // exchange key — not the Ed25519 identity key.
        let (ratchet, is_initiator) = session.build_exchange_ratchet(&contact)?;
        storage.save_ratchet_state(&contact_id, &ratchet, is_initiator)?;

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }

    // === Multi-Stage Exchange Operations ===

    /// Create a multi-stage exchange session with the local identity and card.
    ///
    /// Serializes the identity public key and contact card into an exchange
    /// payload that the multi-stage protocol will transfer to the peer.
    pub fn create_multistage_session(&self) -> Result<Arc<MobileMultiStageSession>, MobileError> {
        let identity = self.get_identity()?;
        let card = self.get_own_card_or_default(&identity)?;
        let payload = serialize_exchange_payload(identity.signing_public_key(), &card);
        Ok(Arc::new(MobileMultiStageSession::with_persistence(
            payload,
            self.storage_path.clone(),
            self.storage_key.clone(),
        )))
    }
}
