// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact exchange operations for mobile.

use std::sync::Arc;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;

use super::VauchiPlatform;
use super::error::MobileError;
use super::exchange::MobileExchangeSession;
use super::exchange::MobileProximityHandler;
use super::multistage_exchange::MobileMultiStageSession;
use super::types::MobileExchangeResult;

/// Exchange payload format version byte.
const EXCHANGE_PAYLOAD_VERSION: u8 = 1;

/// Serialize identity public key + contact card into an exchange payload.
///
/// Format: `[version: 1 byte][public_key: 32 bytes][card_json: rest]`
fn serialize_exchange_payload(public_key: &[u8; 32], card: &ContactCard) -> Vec<u8> {
    let card_json = serde_json::to_vec(card).expect("ContactCard serialization should not fail");
    let mut payload = Vec::with_capacity(1 + 32 + card_json.len());
    payload.push(EXCHANGE_PAYLOAD_VERSION);
    payload.extend_from_slice(public_key);
    payload.extend_from_slice(&card_json);
    payload
}

/// Deserialize an exchange payload into (public_key, ContactCard).
fn deserialize_exchange_payload(data: &[u8]) -> Result<([u8; 32], ContactCard), MobileError> {
    if data.len() < 34 {
        return Err(MobileError::ExchangeFailed(
            "Exchange payload too short".to_string(),
        ));
    }
    let version = data[0];
    if version != EXCHANGE_PAYLOAD_VERSION {
        return Err(MobileError::ExchangeFailed(format!(
            "Unsupported exchange payload version: {}",
            version
        )));
    }
    let mut public_key = [0u8; 32];
    public_key.copy_from_slice(&data[1..33]);
    let card: ContactCard = serde_json::from_slice(&data[33..]).map_err(|e| {
        MobileError::ExchangeFailed(format!("Failed to deserialize contact card: {}", e))
    })?;
    Ok((public_key, card))
}

#[uniffi::export]
impl VauchiPlatform {
    // === Exchange Operations ===

    /// Create a QR exchange session with proximity verification.
    ///
    /// Both parties display and scan QR codes. Uses fresh ephemeral keys
    /// for full forward secrecy.
    pub fn create_qr_exchange(
        &self,
        proximity: Box<dyn MobileProximityHandler>,
    ) -> Result<Arc<MobileExchangeSession>, MobileError> {
        let identity = self.get_identity()?;
        let our_card = self.get_own_card_or_default(&identity)?;
        Ok(super::exchange::create_qr_exchange_proximity(
            identity, our_card, proximity,
        ))
    }

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

        // Initialize double ratchet
        // Exchange contacts are always exchanged type
        let shared_key = contact
            .shared_key()
            .expect("exchange contact has shared key")
            .clone();
        let their_exchange_key = *contact
            .public_key()
            .expect("exchange contact has public key");
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_key, their_exchange_key)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

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
        Ok(Arc::new(MobileMultiStageSession::new(payload)))
    }

    /// Finalize a completed multi-stage exchange session.
    ///
    /// Deserializes the peer's exchange payload (public key + contact card),
    /// creates a Contact using the transport key as the shared secret,
    /// saves it to storage, and initializes the double ratchet.
    ///
    /// On repeat exchange: same ratchet-reset semantics as `finalize_exchange` —
    /// card is upserted, ratchet re-initialized, in-flight messages lost.
    ///
    /// The session must be in the Complete state with received data available.
    pub fn finalize_multistage_exchange(
        &self,
        session: &MobileMultiStageSession,
    ) -> Result<MobileExchangeResult, MobileError> {
        let received_data = session
            .get_received_data()
            .ok_or_else(|| MobileError::ExchangeFailed("No received data".to_string()))?;

        let transport_key_bytes: [u8; 32] = session
            .get_transport_key()
            .ok_or_else(|| MobileError::ExchangeFailed("No transport key".to_string()))?
            .try_into()
            .map_err(|_| MobileError::ExchangeFailed("Invalid transport key length".to_string()))?;

        let (public_key, card) = deserialize_exchange_payload(&received_data)?;
        let shared_key = SymmetricKey::from_bytes(transport_key_bytes);

        let contact = Contact::from_exchange(public_key, card, shared_key.clone());
        let storage = self.open_storage()?;

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        // Upsert: save_contact uses INSERT ON CONFLICT UPDATE, so
        // repeated exchanges with the same peer update the card data
        // rather than failing.
        storage.save_contact(&contact)?;

        // Initialize double ratchet with transport-derived shared key
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_key, public_key)
            .map_err(|e| MobileError::ExchangeFailed(e.to_string()))?;
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }
}
