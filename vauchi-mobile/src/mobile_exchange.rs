// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact exchange operations for mobile.

use std::sync::Arc;
use std::time::Duration;

use futures_util::SinkExt;
use tokio_tungstenite::tungstenite::Message;

use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::EncryptedExchangeMessage;

use super::error::MobileError;
use super::exchange::MobileExchangeSession;
use super::exchange::MobileProximityHandler;
use super::types::MobileExchangeResult;
use super::{cert_pinning, protocol, VauchiMobile};

#[uniffi::export]
impl VauchiMobile {
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
    /// initializes the double ratchet, and sends the encrypted exchange message via relay.
    ///
    /// The session must be in the Complete state (i.e., the state machine has been
    /// driven through all steps).
    pub fn finalize_exchange(
        &self,
        session: &MobileExchangeSession,
    ) -> Result<MobileExchangeResult, MobileError> {
        let contact = session.extract_contact()?;
        let identity = self.get_identity()?;
        let storage = self.open_storage()?;

        let contact_id = contact.id().to_string();
        let contact_name = contact.display_name().to_string();

        // Check for duplicate
        if storage.load_contact(&contact_id)?.is_some() {
            return Err(MobileError::ExchangeFailed(
                "Contact already exists".to_string(),
            ));
        }

        // Save contact
        storage.save_contact(&contact)?;

        // Initialize double ratchet
        let shared_key = contact.shared_key().clone();
        let their_exchange_key = *contact.public_key();
        let ratchet = DoubleRatchetState::initialize_initiator(&shared_key, their_exchange_key);
        storage.save_ratchet_state(&contact_id, &ratchet, true)?;

        // Send encrypted exchange message via relay (async, uses block_on)
        {
            let our_x3dh = identity.x3dh_keypair();
            let (encrypted_msg, _) = EncryptedExchangeMessage::create(
                &our_x3dh,
                &their_exchange_key,
                identity.signing_public_key(),
                identity.display_name(),
            )
            .map_err(|e| MobileError::ExchangeFailed(format!("Key agreement failed: {:?}", e)))?;

            let our_id = identity.public_id();
            let pinned_cert = self.get_pinned_cert();
            let relay_url = self.relay_url.clone();

            let update = protocol::EncryptedUpdate {
                recipient_id: contact_id.clone(),
                sender_id: our_id,
                ciphertext: encrypted_msg.to_bytes(),
            };

            let envelope =
                protocol::create_envelope(protocol::MessagePayload::EncryptedUpdate(update));
            let data = protocol::encode_message(&envelope).map_err(MobileError::SyncFailed)?;

            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| MobileError::Internal(format!("Runtime error: {}", e)))?;

            rt.block_on(async {
                let mut socket =
                    cert_pinning::connect_with_pinning(&relay_url, pinned_cert.as_deref())
                        .await
                        .map_err(MobileError::NetworkError)?;

                let handshake =
                    vauchi_core::network::simple_message::create_signed_handshake(&identity, None);
                let hs_envelope =
                    protocol::create_envelope(protocol::MessagePayload::Handshake(handshake));
                let hs_data = protocol::encode_message(&hs_envelope)
                    .map_err(|e| MobileError::SyncFailed(format!("Encode error: {}", e)))?;
                socket
                    .send(Message::Binary(hs_data))
                    .await
                    .map_err(|e| MobileError::NetworkError(e.to_string()))?;

                socket
                    .send(Message::Binary(data))
                    .await
                    .map_err(|e| MobileError::NetworkError(e.to_string()))?;

                tokio::time::sleep(Duration::from_millis(100)).await;
                let _ = socket.close(None).await;

                Ok::<(), MobileError>(())
            })?;
        }

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }
}
