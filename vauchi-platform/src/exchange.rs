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
    ProximityError, ProximityVerifier, VerifierChain, VerifierMethod,
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
        // Safety-net: the mobile handler is invoked at the two_way level.
        // These stubs exist only for trait completeness. If verify_proximity_two_way
        // were accidentally removed, the default impl would call these stubs and
        // fail safely via Err(NotSupported) in the real handler path.
        Err(ProximityError::NotSupported)
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        Err(ProximityError::NotSupported)
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        false
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

    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        _listen_challenge: &[u8; 16],
        timeout: Duration,
        _is_initiator: bool,
    ) -> Result<(), ProximityError> {
        // TODO(security): This delegates only the emit direction. The two-way
        // protocol requires both parties independently verify the other's
        // challenge. The MobileProximityHandler interface needs a two-argument
        // form before this can enforce full bidirectionality.
        self.verify_proximity(emit_challenge, timeout)
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

/// Mobile exchange session wrapping the core `ExchangeSession` state machine.
///
/// Drives the exchange flow: generate/scan QR -> verify proximity -> key agreement -> complete.
/// Since `ExchangeSession` stores `Box<dyn ProximityVerifier>`, no enum dispatch is needed.
#[derive(uniffi::Object)]
pub struct MobileExchangeSession {
    inner: Mutex<ExchangeSession>,
    manual_verifier: Option<Arc<ManualConfirmationVerifier>>,
}

impl MobileExchangeSession {
    /// Create a new session with an optional manual verifier handle.
    pub(crate) fn new(
        session: ExchangeSession,
        manual_verifier: Option<Arc<ManualConfirmationVerifier>>,
    ) -> Self {
        MobileExchangeSession {
            inner: Mutex::new(session),
            manual_verifier,
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

    /// Returns the peer's display name extracted from their QR code.
    ///
    /// Available after `process_qr()` has been called successfully.
    pub fn peer_display_name(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .their_display_name()
            .map(String::from)
    }

    /// Returns the proximity confidence from the last verification.
    ///
    /// Available after key agreement has been performed.
    pub fn verification_confidence(
        &self,
    ) -> crate::mobile_verifier_event::MobileProximityConfidence {
        self.inner.lock().unwrap().proximity_confidence().into()
    }

    /// Enable exchange debug logging.
    ///
    /// When enabled, captures timestamped events at each state transition
    /// (QR generation, scan, key agreement, proximity, completion/failure).
    /// Call before starting the exchange flow.
    pub fn enable_debug_log(&self) {
        self.inner.lock().unwrap().enable_debug_log();
    }

    /// Returns the exchange debug log as JSONL, if enabled.
    pub fn get_exchange_debug_jsonl(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .exchange_debug_log()
            .map(|log| log.to_jsonl())
    }

    /// Returns the exchange debug log as Markdown, if enabled.
    pub fn get_exchange_debug_markdown(&self) -> Option<String> {
        self.inner
            .lock()
            .unwrap()
            .exchange_debug_log()
            .map(|log| log.to_markdown())
    }

    /// Returns the event log from the last proximity verification.
    ///
    /// Returns an empty list before any verification has occurred.
    /// After key agreement, contains the chain's events (InProgress,
    /// Completed, MethodFailed, FallingBack, etc.).
    pub fn get_verification_events(
        &self,
    ) -> Vec<crate::mobile_verifier_event::MobileProximityVerifierEvent> {
        self.inner
            .lock()
            .unwrap()
            .proximity_event_log()
            .map(|log| log.events().iter().cloned().map(Into::into).collect())
            .unwrap_or_default()
    }
}

// === Factory Functions ===

/// Create a QR exchange session with proximity verification.
///
/// Wraps the proximity bridge in a single-entry `VerifierChain` so that
/// verification events are always available via `get_verification_events()`.
pub(crate) fn create_qr_exchange_proximity(
    identity: Identity,
    our_card: ContactCard,
    handler: Box<dyn MobileProximityHandler>,
) -> Arc<MobileExchangeSession> {
    let bridge = ProximityBridge {
        handler: Arc::from(handler),
    };
    let mut chain = VerifierChain::new();
    // TODO(method-label): Method label is hardcoded to Ultrasonic — MobileProximityHandler
    // may actually use BLE or another mechanism. Revisit when the handler interface
    // reports its actual method.
    chain.add(VerifierMethod::Ultrasonic, Box::new(bridge));
    let session = ExchangeSession::new_qr(identity, our_card, chain);
    Arc::new(MobileExchangeSession::new(session, None))
}

/// Create a QR exchange session with manual confirmation.
///
/// Wraps the manual bridge in a single-entry `VerifierChain` so that
/// verification events are always available via `get_verification_events()`.
pub(crate) fn create_qr_exchange_manual(
    identity: Identity,
    our_card: ContactCard,
) -> Arc<MobileExchangeSession> {
    let (bridge, verifier) = ManualConfirmationBridge::new();
    let mut chain = VerifierChain::new();
    chain.add(VerifierMethod::ManualConfirmation, Box::new(bridge));
    let session = ExchangeSession::new_qr(identity, our_card, chain);
    Arc::new(MobileExchangeSession::new(session, Some(verifier)))
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

    // === Phase B: Event wiring tests ===

    #[test]
    fn test_verification_confidence_defaults_to_unknown() {
        use crate::mobile_verifier_event::MobileProximityConfidence;

        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        // Before any verification, confidence is Unknown
        assert_eq!(
            session.verification_confidence(),
            MobileProximityConfidence::Unknown
        );
    }

    #[test]
    fn test_get_verification_events_empty_before_verification() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        // Before any verification, events list is empty
        assert!(session.get_verification_events().is_empty());
    }

    #[test]
    fn test_verification_events_populated_after_key_agreement() {
        use crate::mobile_verifier_event::{
            MobileProximityConfidence, MobileProximityVerifierEvent,
        };

        let alice = Identity::create("Alice");
        let alice_card = ContactCard::new("Alice");
        let bob = Identity::create("Bob");
        let bob_card = ContactCard::new("Bob");

        // Create manual sessions (confirmed before key agreement so verification succeeds)
        let alice_session = {
            let verifier = ManualConfirmationVerifier::new();
            verifier.confirm();
            let mut chain = VerifierChain::new();
            chain.add(VerifierMethod::ManualConfirmation, Box::new(verifier));
            let session = ExchangeSession::new_qr(alice, alice_card, chain);
            Arc::new(MobileExchangeSession::new(session, None))
        };
        let bob_session = {
            let verifier = ManualConfirmationVerifier::new();
            verifier.confirm();
            let mut chain = VerifierChain::new();
            chain.add(VerifierMethod::ManualConfirmation, Box::new(verifier));
            let session = ExchangeSession::new_qr(bob, bob_card, chain);
            Arc::new(MobileExchangeSession::new(session, None))
        };

        // Drive through exchange to key agreement
        let alice_qr = alice_session.generate_qr().unwrap();
        let bob_qr = bob_session.generate_qr().unwrap();
        alice_session.process_qr(bob_qr).unwrap();
        bob_session.process_qr(alice_qr).unwrap();
        alice_session.they_scanned_our_qr().unwrap();
        bob_session.they_scanned_our_qr().unwrap();

        // Key agreement triggers proximity verification
        alice_session.perform_key_agreement().unwrap();
        bob_session.perform_key_agreement().unwrap();

        // Events should now be populated for both sides
        let alice_events = alice_session.get_verification_events();
        assert!(
            !alice_events.is_empty(),
            "Alice events should be populated after key agreement"
        );
        let bob_events = bob_session.get_verification_events();
        assert!(
            !bob_events.is_empty(),
            "Bob events should be populated after key agreement"
        );

        // Both should contain a Completed event with Medium confidence
        // (ManualConfirmation verifier reports Medium)
        let alice_completed = alice_events.iter().find(|e| {
            matches!(
                e,
                MobileProximityVerifierEvent::Completed {
                    confidence: MobileProximityConfidence::Medium,
                    ..
                }
            )
        });
        assert!(
            alice_completed.is_some(),
            "Alice should have a Completed event with Medium confidence"
        );

        let bob_completed = bob_events.iter().find(|e| {
            matches!(
                e,
                MobileProximityVerifierEvent::Completed {
                    confidence: MobileProximityConfidence::Medium,
                    ..
                }
            )
        });
        assert!(
            bob_completed.is_some(),
            "Bob should have a Completed event with Medium confidence"
        );
    }

    #[test]
    fn test_proximity_factory_wraps_in_chain() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_proximity(identity, card, Box::new(SuccessHandler));

        // Events start empty (no verification yet)
        assert!(session.get_verification_events().is_empty());
    }

    #[test]
    fn test_manual_factory_wraps_in_chain() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        // Events start empty (no verification yet)
        assert!(session.get_verification_events().is_empty());
    }

    // === Debug log wiring tests ===

    #[test]
    fn test_debug_log_disabled_by_default() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        assert!(session.get_exchange_debug_jsonl().is_none());
        assert!(session.get_exchange_debug_markdown().is_none());
    }

    #[test]
    fn test_enable_debug_log_captures_events() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        session.enable_debug_log();
        session.generate_qr().unwrap();

        let jsonl = session
            .get_exchange_debug_jsonl()
            .expect("log should exist");
        let lines: Vec<&str> = jsonl.lines().collect();
        // SessionStarted + QrGenerated
        assert_eq!(lines.len(), 2);
        assert!(jsonl.contains("session_started"));
        assert!(jsonl.contains("qr_generated"));
    }

    #[test]
    fn test_debug_log_markdown_output() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let session = create_qr_exchange_manual(identity, card);

        session.enable_debug_log();
        session.generate_qr().unwrap();

        let md = session
            .get_exchange_debug_markdown()
            .expect("log should exist");
        assert!(md.contains("# Exchange Debug Log"));
        assert!(md.contains("2 events"));
        assert!(md.contains("SessionStarted"));
        assert!(md.contains("QrGenerated"));
    }
}
