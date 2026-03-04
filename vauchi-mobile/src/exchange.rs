// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Session Mobile Bindings
//!
//! Wraps vauchi-core's `ExchangeSession` state machine for mobile platforms.
//! Provides a callback interface for proximity verification and a UniFFI object
//! that mobile apps drive through the exchange flow.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeQR, ExchangeSession, ExchangeState, ManualConfirmationVerifier,
    ProximityError, ProximityVerifier,
};
use vauchi_core::identity::Identity;

use crate::error::MobileError;

// === Callback Interface ===

/// Callback interface for platform-specific proximity verification.
///
/// Mobile apps (iOS/Android) implement this to provide proximity verification
/// using ultrasonic audio or other hardware-based mechanisms.
#[uniffi::export(callback_interface)]
pub trait MobileProximityHandler: Send + Sync {
    /// Perform proximity verification with the given challenge.
    ///
    /// challenge: 16 bytes from the QR code's audio_challenge field.
    /// timeout_ms: maximum time to wait in milliseconds.
    ///
    /// Returns empty string on success, error message on failure.
    fn verify_proximity(&self, challenge: Vec<u8>, timeout_ms: u64) -> String;
}

// === ProximityBridge ===

/// Adapts a `MobileProximityHandler` callback to vauchi-core's `ProximityVerifier` trait.
pub(crate) struct ProximityBridge {
    handler: Arc<dyn MobileProximityHandler>,
}

impl ProximityVerifier for ProximityBridge {
    fn confidence_level(&self) -> vauchi_core::exchange::ProximityConfidence {
        vauchi_core::exchange::ProximityConfidence::High
    }

    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        Ok(())
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        Ok(vec![0x01])
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        true
    }

    fn verify_proximity(
        &self,
        challenge: &[u8; 16],
        timeout: Duration,
    ) -> Result<(), ProximityError> {
        let result = self
            .handler
            .verify_proximity(challenge.to_vec(), timeout.as_millis() as u64);
        if result.is_empty() {
            Ok(())
        } else {
            Err(ProximityError::DeviceError(result))
        }
    }
}

// === ManualConfirmationBridge ===

/// Wraps `ManualConfirmationVerifier` for devices without audio hardware.
///
/// Created internally — no callback interface needed. The mobile app calls
/// `confirm_proximity()` on the session, which sets the confirmation flag
/// before the state machine checks it.
pub(crate) struct ManualConfirmationBridge {
    inner: Arc<ManualConfirmationVerifier>,
}

impl ManualConfirmationBridge {
    fn new() -> (Self, Arc<ManualConfirmationVerifier>) {
        let verifier = Arc::new(ManualConfirmationVerifier::new());
        (
            Self {
                inner: verifier.clone(),
            },
            verifier,
        )
    }
}

impl ProximityVerifier for ManualConfirmationBridge {
    fn confidence_level(&self) -> vauchi_core::exchange::ProximityConfidence {
        self.inner.confidence_level()
    }

    fn emit_challenge(&self, challenge: &[u8; 16]) -> Result<(), ProximityError> {
        self.inner.emit_challenge(challenge)
    }

    fn listen_for_response(&self, timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        self.inner.listen_for_response(timeout)
    }

    fn verify_response(&self, challenge: &[u8; 16], response: &[u8]) -> bool {
        self.inner.verify_response(challenge, response)
    }

    fn verify_proximity(
        &self,
        challenge: &[u8; 16],
        timeout: Duration,
    ) -> Result<(), ProximityError> {
        self.inner.verify_proximity(challenge, timeout)
    }
}

// === Mobile-Friendly State Enum ===

/// Mobile-friendly exchange state (no raw bytes or core types).
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileExchangeState {
    Idle,
    DisplayingQr {
        qr_data: String,
    },
    PeerScanned,
    AwaitingKeyAgreement,
    AwaitingCardExchange,
    Complete {
        contact_id: String,
        contact_name: String,
    },
    Failed {
        error: String,
    },
}

/// Status of BLE exchange availability on this device.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileBleExchangeStatus {
    /// BLE exchange is available (native transport configured)
    Available,
    /// BLE exchange not available — requires native Bluetooth implementation
    NotAvailable { reason: String },
}

// === Session Wrapper ===

/// Internal enum to hold either type of session.
enum SessionInner {
    Proximity(ExchangeSession<ProximityBridge>),
    Manual(ExchangeSession<ManualConfirmationBridge>),
}

impl SessionInner {
    fn state(&self) -> &ExchangeState {
        match self {
            SessionInner::Proximity(s) => s.state(),
            SessionInner::Manual(s) => s.state(),
        }
    }

    fn apply(&mut self, event: ExchangeEvent) -> Result<(), vauchi_core::exchange::ExchangeError> {
        match self {
            SessionInner::Proximity(s) => s.apply(event),
            SessionInner::Manual(s) => s.apply(event),
        }
    }

    fn is_timed_out(&self) -> bool {
        match self {
            SessionInner::Proximity(s) => s.is_timed_out(),
            SessionInner::Manual(s) => s.is_timed_out(),
        }
    }

    fn qr(&self) -> Option<&ExchangeQR> {
        match self {
            SessionInner::Proximity(s) => s.qr(),
            SessionInner::Manual(s) => s.qr(),
        }
    }
}

/// Mobile exchange session wrapping the core `ExchangeSession` state machine.
///
/// Drives the exchange flow: generate/scan QR -> verify proximity -> key agreement -> complete.
#[derive(uniffi::Object)]
pub struct MobileExchangeSession {
    inner: Mutex<SessionInner>,
    manual_verifier: Option<Arc<ManualConfirmationVerifier>>,
}

impl MobileExchangeSession {
    /// Create a new session from a proximity-based inner session.
    pub(crate) fn from_proximity(session: ExchangeSession<ProximityBridge>) -> Self {
        MobileExchangeSession {
            inner: Mutex::new(SessionInner::Proximity(session)),
            manual_verifier: None,
        }
    }

    /// Create a new session from a manual-confirmation inner session.
    pub(crate) fn from_manual(
        session: ExchangeSession<ManualConfirmationBridge>,
        verifier: Arc<ManualConfirmationVerifier>,
    ) -> Self {
        MobileExchangeSession {
            inner: Mutex::new(SessionInner::Manual(session)),
            manual_verifier: Some(verifier),
        }
    }

    /// Extract the contact from a completed session (used by finalize_exchange).
    pub(crate) fn extract_contact(&self) -> Result<Contact, MobileError> {
        let inner = self.inner.lock().unwrap();
        match inner.state() {
            ExchangeState::Complete { contact } => Ok(contact.clone()),
            _ => Err(MobileError::ExchangeFailed(
                "Session not in Complete state — drive the state machine first".into(),
            )),
        }
    }
}

#[uniffi::export]
impl MobileExchangeSession {
    /// Get the current state of the exchange session.
    pub fn state(&self) -> MobileExchangeState {
        let inner = self.inner.lock().unwrap();
        match inner.state() {
            ExchangeState::Idle => MobileExchangeState::Idle,
            ExchangeState::DisplayingQr { our_qr } => MobileExchangeState::DisplayingQr {
                qr_data: format!("wb://{}", our_qr.to_data_string()),
            },
            ExchangeState::PeerScanned { .. } => MobileExchangeState::PeerScanned,
            ExchangeState::AwaitingKeyAgreement { .. } => MobileExchangeState::AwaitingKeyAgreement,
            ExchangeState::AwaitingCardExchange { .. } => MobileExchangeState::AwaitingCardExchange,
            ExchangeState::Complete { contact } => MobileExchangeState::Complete {
                contact_id: contact.id().to_string(),
                contact_name: contact.display_name().to_string(),
            },
            ExchangeState::Failed { error } => MobileExchangeState::Failed {
                error: format!("{:?}", error),
            },
            ExchangeState::AwaitingNfcTap => MobileExchangeState::Idle,
            ExchangeState::AwaitingBleConnection => MobileExchangeState::Idle,
            ExchangeState::AwaitingBleVerification { .. } => MobileExchangeState::Idle,
        }
    }

    /// Generate and display a QR code. Transitions Idle -> DisplayingQr.
    pub fn generate_qr(&self) -> Result<String, MobileError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .apply(ExchangeEvent::StartQR)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))?;

        // Return the QR data string
        inner
            .qr()
            .map(|qr| format!("wb://{}", qr.to_data_string()))
            .ok_or_else(|| MobileError::ExchangeFailed("QR not generated".into()))
    }

    /// Process a scanned QR code. Transitions DisplayingQr -> PeerScanned.
    pub fn process_qr(&self, qr_data: String) -> Result<(), MobileError> {
        let data_str = qr_data.strip_prefix("wb://").unwrap_or(&qr_data);
        let qr = ExchangeQR::from_data_string(data_str).map_err(|_| MobileError::InvalidQrCode)?;

        let mut inner = self.inner.lock().unwrap();
        inner
            .apply(ExchangeEvent::ProcessQR(qr))
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Signal that the other party scanned our QR. Transitions PeerScanned -> AwaitingKeyAgreement.
    pub fn they_scanned_our_qr(&self) -> Result<(), MobileError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .apply(ExchangeEvent::TheyScannedOurQR)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Perform key agreement. Transitions AwaitingKeyAgreement -> AwaitingCardExchange.
    pub fn perform_key_agreement(&self) -> Result<(), MobileError> {
        let mut inner = self.inner.lock().unwrap();
        inner
            .apply(ExchangeEvent::PerformKeyAgreement)
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Complete the card exchange. Transitions AwaitingCardExchange -> Complete.
    ///
    /// The `their_card_name` is used to create a placeholder card for the contact.
    /// The real card will be received via relay sync.
    pub fn complete_card_exchange(&self, their_card_name: String) -> Result<(), MobileError> {
        let card = ContactCard::new(&their_card_name);
        let mut inner = self.inner.lock().unwrap();
        inner
            .apply(ExchangeEvent::CompleteExchange(card))
            .map_err(|e| MobileError::ExchangeFailed(format!("{:?}", e)))
    }

    /// Confirm physical proximity (manual verification).
    ///
    /// For manual sessions: sets the confirmation flag so the exchange
    /// can proceed. Call this after the user confirms they are physically
    /// present with the other party.
    ///
    /// For proximity sessions: no-op (auto-verified via audio hardware).
    pub fn confirm_proximity(&self) -> Result<(), MobileError> {
        if let Some(verifier) = &self.manual_verifier {
            verifier.confirm();
        }
        Ok(())
    }

    /// Check if the session has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.inner.lock().unwrap().is_timed_out()
    }
}

// === Factory Functions ===

/// Create a QR exchange session with proximity verification.
pub(crate) fn create_qr_exchange_proximity(
    identity: Identity,
    our_card: ContactCard,
    handler: Box<dyn MobileProximityHandler>,
) -> Arc<MobileExchangeSession> {
    let bridge = ProximityBridge {
        handler: Arc::from(handler),
    };
    let session = ExchangeSession::new_qr(identity, our_card, bridge);
    Arc::new(MobileExchangeSession::from_proximity(session))
}

/// Create a QR exchange session with manual confirmation.
pub(crate) fn create_qr_exchange_manual(
    identity: Identity,
    our_card: ContactCard,
) -> Arc<MobileExchangeSession> {
    let (bridge, verifier) = ManualConfirmationBridge::new();
    let session = ExchangeSession::new_qr(identity, our_card, bridge);
    Arc::new(MobileExchangeSession::from_manual(session, verifier))
}

/// Check if BLE exchange is available on this device.
///
/// Returns NotAvailable until native BLE transport is implemented
/// for each platform (CoreBluetooth on iOS, Android BLE on Android).
#[uniffi::export]
pub fn ble_exchange_status() -> MobileBleExchangeStatus {
    MobileBleExchangeStatus::NotAvailable {
        reason: "Native Bluetooth transport not yet implemented".into(),
    }
}

// ============================================================
// BLE Mesh Exchange Bindings
// ============================================================

/// Callback interface for platform-specific BLE operations.
///
/// Mobile apps (iOS/Android) implement this to provide BLE transport
/// using platform-native APIs (CoreBluetooth on iOS, Android BLE on Android).
#[uniffi::export(callback_interface)]
pub trait MobileBLEHandler: Send + Sync {
    /// Start BLE advertising with the given service data.
    ///
    /// service_data: serialized MeshAdvertisement bytes (16-byte session ID).
    /// Returns empty string on success, error message on failure.
    fn start_advertising(&self, service_data: Vec<u8>) -> String;

    /// Start BLE scanning for nearby Vauchi devices.
    ///
    /// Returns empty string on success, error message on failure.
    fn start_scanning(&self) -> String;

    /// Stop all BLE advertising and scanning.
    fn stop(&self);

    /// Connect to a discovered BLE device by ID.
    ///
    /// Returns empty string on success, error message on failure.
    fn connect(&self, device_id: String) -> String;

    /// Write data to a GATT characteristic.
    ///
    /// Returns empty string on success, error message on failure.
    fn write_characteristic(&self, uuid: String, data: Vec<u8>) -> String;

    /// Read data from a GATT characteristic.
    ///
    /// Returns the read data on success, or empty vec on failure.
    fn read_characteristic(&self, uuid: String) -> Vec<u8>;

    /// Disconnect from the currently connected device.
    fn disconnect(&self);

    /// Returns the list of discovered device IDs since last scan start.
    ///
    /// Each entry is "device_id:rssi" (e.g., "ABC123:-55").
    fn discovered_devices(&self) -> Vec<String>;
}

/// Mobile-friendly mesh exchange state.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileMeshState {
    /// Mesh is not active.
    Disabled,
    /// Actively scanning and advertising.
    Active {
        /// Number of discovered peers.
        discovered_count: u32,
    },
    /// Exchanging with a specific peer.
    Exchanging {
        /// Device ID of the peer.
        peer_id: String,
    },
    /// Temporarily paused.
    Paused,
}

/// Check if mesh BLE exchange is available on this device.
///
/// Returns NotAvailable until a MobileBLEHandler is registered.
#[uniffi::export]
pub fn mesh_exchange_status() -> MobileBleExchangeStatus {
    MobileBleExchangeStatus::NotAvailable {
        reason: "Native Bluetooth transport not yet implemented".into(),
    }
}

// === Tests ===

// INLINE_TEST_REQUIRED: tests use private ProximityBridge internals and create_qr_exchange_manual/proximity helpers
#[cfg(test)]
mod tests {
    use super::*;

    /// Mock proximity handler that always succeeds.
    struct SuccessHandler;
    impl MobileProximityHandler for SuccessHandler {
        fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
            String::new()
        }
    }

    /// Mock proximity handler that always fails.
    struct FailureHandler;
    impl MobileProximityHandler for FailureHandler {
        fn verify_proximity(&self, _challenge: Vec<u8>, _timeout_ms: u64) -> String {
            "Device too far away".to_string()
        }
    }

    // @scenario: contact_exchange:Successful QR code exchange with proximity
    #[test]
    fn test_proximity_bridge_success() {
        let handler = Arc::new(SuccessHandler);
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let challenge = [0xAA; 16];
        let result = bridge.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_ok());
    }

    // @scenario: contact_exchange:QR code exchange blocked without proximity
    #[test]
    fn test_proximity_bridge_failure() {
        let handler = Arc::new(FailureHandler);
        let bridge = ProximityBridge {
            handler: handler.clone(),
        };

        let challenge = [0xBB; 16];
        let result = bridge.verify_proximity(&challenge, Duration::from_secs(5));
        assert!(result.is_err());
        match result.unwrap_err() {
            ProximityError::DeviceError(msg) => assert_eq!(msg, "Device too far away"),
            other => panic!("Expected DeviceError, got {:?}", other),
        }
    }

    // @scenario: contact_exchange:Generate exchange QR code
    #[test]
    fn test_session_generates_qr() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");

        let session = create_qr_exchange_manual(identity, card);

        // Initially idle
        assert!(matches!(session.state(), MobileExchangeState::Idle));

        // Generate QR
        let qr_data = session.generate_qr().unwrap();
        assert!(qr_data.starts_with("wb://"));

        // State should be DisplayingQr
        assert!(matches!(
            session.state(),
            MobileExchangeState::DisplayingQr { .. }
        ));
    }

    // @scenario: contact_exchange:Mutual QR exchange with bidirectional scanning
    #[test]
    fn test_session_mutual_qr_flow() {
        let alice = Identity::create("Alice");
        let alice_card = ContactCard::new("Alice");
        let bob = Identity::create("Bob");
        let bob_card = ContactCard::new("Bob");

        // Both sides create QR sessions and generate QR codes
        let alice_session = create_qr_exchange_manual(alice, alice_card);
        let alice_qr = alice_session.generate_qr().unwrap();

        let bob_session = create_qr_exchange_manual(bob, bob_card);
        let bob_qr = bob_session.generate_qr().unwrap();

        // Both scan each other's QR
        alice_session.process_qr(bob_qr).unwrap();
        bob_session.process_qr(alice_qr).unwrap();

        // Both should be in PeerScanned
        assert!(matches!(
            alice_session.state(),
            MobileExchangeState::PeerScanned
        ));
        assert!(matches!(
            bob_session.state(),
            MobileExchangeState::PeerScanned
        ));

        // Signal that the other party scanned our QR
        alice_session.they_scanned_our_qr().unwrap();
        bob_session.they_scanned_our_qr().unwrap();

        assert!(matches!(
            alice_session.state(),
            MobileExchangeState::AwaitingKeyAgreement
        ));

        // Key agreement
        alice_session.perform_key_agreement().unwrap();
        bob_session.perform_key_agreement().unwrap();

        assert!(matches!(
            alice_session.state(),
            MobileExchangeState::AwaitingCardExchange
        ));

        // Complete card exchange
        alice_session
            .complete_card_exchange("Bob".to_string())
            .unwrap();
        bob_session
            .complete_card_exchange("Alice".to_string())
            .unwrap();

        assert!(matches!(
            alice_session.state(),
            MobileExchangeState::Complete { .. }
        ));
        assert!(matches!(
            bob_session.state(),
            MobileExchangeState::Complete { .. }
        ));
    }

    // @scenario: contact_exchange:Incomplete exchange recovery
    #[test]
    fn test_finalize_requires_complete_state() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");

        let session = create_qr_exchange_manual(identity, card);

        // Should fail — session is Idle, not Complete
        let result = session.extract_contact();
        assert!(result.is_err());
    }

    // @scenario: contact_exchange:Successful QR code exchange with proximity
    #[test]
    fn test_confirm_proximity_manual_session() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");

        let session = create_qr_exchange_manual(identity, card);
        // Should succeed without error
        session
            .confirm_proximity()
            .expect("confirm_proximity should succeed on manual session");
    }

    // @scenario: contact_exchange:Successful QR code exchange with proximity
    #[test]
    fn test_confirm_proximity_proximity_session() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");

        let session = create_qr_exchange_proximity(identity, card, Box::new(SuccessHandler));
        // Should be no-op for proximity sessions (auto-verified)
        session
            .confirm_proximity()
            .expect("confirm_proximity should be a no-op on proximity session");
    }

    // @scenario: contact_exchange:Exchange timeout after interruption
    #[test]
    fn test_session_not_timed_out_initially() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");

        let session = create_qr_exchange_manual(identity, card);
        assert!(!session.is_timed_out());
    }
}
