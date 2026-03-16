// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Exchange Mobile Bindings
//!
//! Wraps vauchi-core's `NfcHandshakeSession` for mobile platforms.
//! Provides a callback interface for platform NFC transports (CoreNFC, Android HCE)
//! and a UniFFI object that mobile apps drive through the three-phase handshake.

use std::sync::{Arc, Mutex};

use vauchi_core::exchange::{
    ExchangeError, NfcExchangeResult, NfcHandshakeSession, NfcHandshakeState,
};
use vauchi_core::identity::Identity;

use crate::error::MobileError;
use crate::VauchiPlatform;

// === Callback Interface ===

/// Callback interface for platform-specific NFC transport.
///
/// iOS implements this with CoreNFC (reader-only).
/// Android implements this with HCE (card emulation) + NfcAdapter (reader).
#[uniffi::export(callback_interface)]
pub trait MobileNfcTransport: Send + Sync {
    /// Send an APDU command and receive the response.
    ///
    /// Used by the reader side (iOS CoreNFC, Android NfcAdapter).
    /// Returns the response bytes, or an error message.
    fn transceive(&self, command: Vec<u8>) -> Result<Vec<u8>, MobileNfcTransportError>;

    /// Send a response to an incoming APDU command.
    ///
    /// Used by the card-emulation side (Android HCE).
    /// Returns an error message if the response could not be sent.
    fn respond(&self, response: Vec<u8>) -> Result<(), MobileNfcTransportError>;

    /// Check if the NFC session is still active (tag not lost).
    fn is_connected(&self) -> bool;
}

/// Error type for NFC transport callback interface.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileNfcTransportError {
    #[error("NFC transport error: {msg}")]
    TransportFailed { msg: String },

    #[error("NFC tag lost")]
    TagLost,
}

// === Mobile-Friendly State Enum ===

/// Mobile-friendly NFC handshake state.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileNfcState {
    Idle,
    KeyOfferSent,
    KeyAckReceived,
    PayloadSent,
    Complete {
        local_display_name: String,
        remote_display_name: String,
    },
    Failed {
        error: String,
    },
    RelayFallback,
}

/// Result of a completed NFC exchange, exposed to mobile.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileNfcExchangeResult {
    pub remote_identity_key: Vec<u8>,
    pub remote_display_name: String,
    pub remote_exchange_key: Vec<u8>,
    pub local_display_name: String,
}

/// Result from process_key_offer containing both ack and encrypted card.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileNfcKeyAckResult {
    pub key_ack_bytes: Vec<u8>,
    pub encrypted_card_bytes: Vec<u8>,
}

// === Session Wrapper ===

/// Mobile NFC handshake session wrapping the core `NfcHandshakeSession`.
///
/// Drives the three-phase encrypted NFC exchange:
/// Phase 1: create_key_offer / process_key_offer
/// Phase 2: process_key_ack / process_encrypted_card
/// Phase 3: confirm_send_success
#[derive(uniffi::Object)]
pub struct MobileNfcHandshake {
    inner: Mutex<NfcHandshakeSession>,
    identity: Mutex<Identity>,
}

#[uniffi::export]
impl MobileNfcHandshake {
    /// Get the current state of the handshake.
    pub fn state(&self) -> MobileNfcState {
        let inner = self.inner.lock().unwrap();
        map_state(inner.state())
    }

    /// Phase 1 (Initiator): Create key offer APDU payload.
    pub fn create_key_offer(&self) -> Result<Vec<u8>, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        let identity = self.identity.lock().unwrap();
        inner
            .create_key_offer(&identity)
            .map_err(exchange_error_to_mobile)
    }

    /// Phase 2 (Responder): Process incoming key offer, return ack + encrypted card.
    pub fn process_key_offer(
        &self,
        their_offer_bytes: Vec<u8>,
    ) -> Result<MobileNfcKeyAckResult, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        let identity = self.identity.lock().unwrap();
        let (ack_bytes, encrypted_card) = inner
            .process_key_offer(&identity, &their_offer_bytes)
            .map_err(exchange_error_to_mobile)?;
        Ok(MobileNfcKeyAckResult {
            key_ack_bytes: ack_bytes,
            encrypted_card_bytes: encrypted_card,
        })
    }

    /// Phase 2 (Initiator): Process key ack + encrypted card from responder.
    ///
    /// Returns our encrypted card bytes for Phase 3.
    pub fn process_key_ack(
        &self,
        their_ack_bytes: Vec<u8>,
        their_encrypted_card: Vec<u8>,
    ) -> Result<Vec<u8>, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .process_key_ack(&their_ack_bytes, &their_encrypted_card)
            .map_err(exchange_error_to_mobile)
    }

    /// Phase 3 (Responder): Process encrypted card from initiator.
    pub fn process_encrypted_card(
        &self,
        their_encrypted_card: Vec<u8>,
    ) -> Result<MobileNfcExchangeResult, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        let result = inner
            .process_encrypted_card(&their_encrypted_card)
            .map_err(exchange_error_to_mobile)?;
        Ok(nfc_result_to_mobile(&result))
    }

    /// Confirm that Phase 3 send succeeded (Initiator).
    pub fn confirm_send_success(&self) -> Result<MobileNfcExchangeResult, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        let result = inner
            .confirm_send_success()
            .map_err(exchange_error_to_mobile)?;
        Ok(nfc_result_to_mobile(&result))
    }

    /// Enter relay fallback mode when NFC tap drops mid-exchange.
    ///
    /// Returns the exchange_id bytes (for relay routing).
    pub fn enter_relay_fallback(&self) -> Result<Vec<u8>, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        let (exchange_id, _shared_key) = inner
            .enter_relay_fallback()
            .map_err(exchange_error_to_mobile)?;
        Ok(exchange_id.to_vec())
    }
}

// === VauchiPlatform Factory Methods ===

#[uniffi::export]
impl VauchiPlatform {
    /// Create an NFC exchange session as the initiator (reader side).
    ///
    /// Used by iOS (CoreNFC reader) and Android (NfcAdapter reader).
    pub fn create_nfc_initiator(&self) -> Result<Arc<MobileNfcHandshake>, MobileError> {
        let identity = self.get_identity()?;
        let display_name = identity.display_name().to_string();
        Ok(create_nfc_session(identity, display_name, true))
    }

    /// Create an NFC exchange session as the responder (HCE side).
    ///
    /// Used by Android only (HostApduService card emulation).
    pub fn create_nfc_responder(&self) -> Result<Arc<MobileNfcHandshake>, MobileError> {
        let identity = self.get_identity()?;
        let display_name = identity.display_name().to_string();
        Ok(create_nfc_session(identity, display_name, false))
    }
}

// === Internal Factory ===

fn create_nfc_session(
    identity: Identity,
    display_name: String,
    is_initiator: bool,
) -> Arc<MobileNfcHandshake> {
    let session = if is_initiator {
        NfcHandshakeSession::new_initiator(&identity, display_name)
    } else {
        NfcHandshakeSession::new_responder(&identity, display_name)
    };
    Arc::new(MobileNfcHandshake {
        inner: Mutex::new(session),
        identity: Mutex::new(identity),
    })
}

#[cfg(test)]
fn create_nfc_initiator_test(identity: Identity, display_name: String) -> Arc<MobileNfcHandshake> {
    create_nfc_session(identity, display_name, true)
}

#[cfg(test)]
fn create_nfc_responder_test(identity: Identity, display_name: String) -> Arc<MobileNfcHandshake> {
    create_nfc_session(identity, display_name, false)
}

// === Helpers ===

fn map_state(state: &NfcHandshakeState) -> MobileNfcState {
    match state {
        NfcHandshakeState::Idle => MobileNfcState::Idle,
        NfcHandshakeState::KeyOfferSent { .. } => MobileNfcState::KeyOfferSent,
        NfcHandshakeState::KeyAckReceived { .. } => MobileNfcState::KeyAckReceived,
        NfcHandshakeState::PayloadSent { .. } => MobileNfcState::PayloadSent,
        NfcHandshakeState::Complete {
            local_card,
            remote_card,
        } => MobileNfcState::Complete {
            local_display_name: local_card.display_name.clone(),
            remote_display_name: remote_card.display_name.clone(),
        },
        NfcHandshakeState::Failed { reason } => MobileNfcState::Failed {
            error: format!("{:?}", reason),
        },
        NfcHandshakeState::RelayFallback { .. } => MobileNfcState::RelayFallback,
    }
}

fn exchange_error_to_mobile(e: ExchangeError) -> MobileError {
    MobileError::ExchangeFailed(format!("{:?}", e))
}

fn nfc_result_to_mobile(result: &NfcExchangeResult) -> MobileNfcExchangeResult {
    MobileNfcExchangeResult {
        remote_identity_key: result.remote_card.identity_key.to_vec(),
        remote_display_name: result.remote_card.display_name.clone(),
        remote_exchange_key: result.remote_card.exchange_key.to_vec(),
        local_display_name: result.local_card.display_name.clone(),
    }
}

// === Tests ===

// INLINE_TEST_REQUIRED: tests use crate-private factory functions
#[cfg(test)]
mod tests {
    use super::*;

    fn make_identity() -> Identity {
        Identity::create("Test")
    }

    #[test]
    fn test_initiator_starts_idle() {
        let session = create_nfc_initiator_test(make_identity(), "Alice".into());
        assert!(matches!(session.state(), MobileNfcState::Idle));
    }

    #[test]
    fn test_responder_starts_idle() {
        let session = create_nfc_responder_test(make_identity(), "Bob".into());
        assert!(matches!(session.state(), MobileNfcState::Idle));
    }

    #[test]
    fn test_create_key_offer_transitions_state() {
        let session = create_nfc_initiator_test(make_identity(), "Alice".into());
        let offer = session.create_key_offer().unwrap();
        assert!(!offer.is_empty());
        assert!(matches!(session.state(), MobileNfcState::KeyOfferSent));
    }

    #[test]
    fn test_full_handshake_via_mobile_api() {
        let alice = create_nfc_initiator_test(Identity::create("Alice"), "Alice".into());
        let bob = create_nfc_responder_test(Identity::create("Bob"), "Bob".into());

        // Phase 1: Alice creates key offer
        let offer = alice.create_key_offer().unwrap();

        // Phase 2: Bob processes offer
        let ack_result = bob.process_key_offer(offer).unwrap();

        // Phase 2 (Alice): Process ack
        let alice_card = alice
            .process_key_ack(ack_result.key_ack_bytes, ack_result.encrypted_card_bytes)
            .unwrap();

        // Phase 3: Bob processes Alice's encrypted card
        let bob_result = bob.process_encrypted_card(alice_card).unwrap();

        // Alice confirms send
        let alice_result = alice.confirm_send_success().unwrap();

        assert_eq!(alice_result.remote_display_name, "Bob");
        assert_eq!(bob_result.remote_display_name, "Alice");
        assert_eq!(alice_result.local_display_name, "Alice");
        assert_eq!(bob_result.local_display_name, "Bob");

        assert!(matches!(alice.state(), MobileNfcState::Complete { .. }));
        assert!(matches!(bob.state(), MobileNfcState::Complete { .. }));
    }

    #[test]
    fn test_relay_fallback_from_mobile() {
        let alice = create_nfc_initiator_test(Identity::create("Alice"), "Alice".into());
        let bob = create_nfc_responder_test(Identity::create("Bob"), "Bob".into());

        let offer = alice.create_key_offer().unwrap();
        let _ack_result = bob.process_key_offer(offer).unwrap();

        // Bob's tap drops — enter relay fallback
        let exchange_id = bob.enter_relay_fallback().unwrap();
        assert_eq!(exchange_id.len(), 32);
        assert!(matches!(bob.state(), MobileNfcState::RelayFallback));
    }

    #[test]
    fn test_double_key_offer_fails() {
        let session = create_nfc_initiator_test(make_identity(), "Alice".into());
        session.create_key_offer().unwrap();
        let result = session.create_key_offer();
        result.expect_err("expected error");
    }
}
