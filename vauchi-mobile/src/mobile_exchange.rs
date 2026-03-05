// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact exchange operations for mobile.

use std::sync::Arc;

use vauchi_core::crypto::ratchet::DoubleRatchetState;

use super::error::MobileError;
use super::exchange::MobileExchangeSession;
use super::exchange::MobileProximityHandler;
use super::types::MobileExchangeResult;
use super::VauchiMobile;

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
    /// and initializes the double ratchet. No relay notification is sent — face-to-face
    /// exchange completes locally on both devices.
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

        Ok(MobileExchangeResult {
            contact_id,
            contact_name,
            success: true,
            error_message: None,
        })
    }
}
