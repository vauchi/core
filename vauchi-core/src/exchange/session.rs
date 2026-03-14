// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Session State Machine
//!
//! Manages the state of a contact exchange from QR generation through
//! key agreement and card exchange.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::ble_handshake::BleHandshakeSession;
use super::ble_payload::BleCardPayload;
use super::nfc_handshake::NfcHandshakeSession;
use super::{ExchangeError, ExchangeQR, ProximityConfidence, ProximityVerifier, X3DHKeyPair};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::kdf::HKDF;
use crate::diagnostic::exchange_debug::{ExchangeDebugEvent, ExchangeDebugLog};
use crate::identity::Identity;

/// Session timeout duration (60 seconds for resumption).
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

/// Transport mechanism used for this exchange session.
pub use crate::types::ExchangeTransport;

/// State of an exchange session.
#[derive(Debug)]
pub enum ExchangeState {
    /// Initial state
    Idle,
    /// Displaying our QR code, waiting for the other party to scan it.
    DisplayingQr { our_qr: ExchangeQR },
    /// We have scanned their QR; waiting for them to scan ours
    /// (or they already did and we proceed to key agreement).
    PeerScanned {
        our_qr: ExchangeQR,
        their_public_key: [u8; 32],
        their_exchange_key: [u8; 32],
    },
    /// Ready for key agreement (both parties have exchanged keys).
    AwaitingKeyAgreement {
        their_public_key: [u8; 32],
        their_exchange_key: [u8; 32],
    },
    /// Key agreement complete, exchanging cards
    AwaitingCardExchange {
        their_public_key: [u8; 32],
        shared_key: crate::crypto::SymmetricKey,
    },
    /// NFC: waiting for both devices to tap.
    AwaitingNfcTap,
    /// BLE: waiting for GATT connection and payload exchange.
    AwaitingBleConnection,
    /// BLE: payloads exchanged, waiting for proximity verification.
    AwaitingBleVerification {
        their_public_key: [u8; 32],
        their_exchange_key: [u8; 32],
        device_id: String,
    },
    /// Exchange completed successfully
    Complete { contact: Contact },
    /// Exchange failed
    Failed { error: ExchangeError },
}

/// Events that drive the exchange state machine.
#[derive(Debug)]
pub enum ExchangeEvent {
    /// Start a QR exchange (generates our QR with fresh ephemeral).
    StartQR,
    /// We scanned their QR code.
    ProcessQR(ExchangeQR),
    /// The other party confirmed they scanned our QR (signal to proceed).
    TheyScannedOurQR,
    /// Perform cryptographic key agreement.
    PerformKeyAgreement,
    /// Exchange contact cards and complete the session.
    CompleteExchange(ContactCard),
    /// Explicitly fail the session.
    Fail(ExchangeError),

    // --- NFC events ---
    /// NFC tap completed; contains their payload bytes.
    NfcTapComplete { their_payload: Vec<u8> },

    // --- BLE events ---
    /// Start a BLE exchange (begin advertising/scanning).
    StartBleExchange,
    /// BLE payloads exchanged; contains their payload bytes and device ID.
    BlePayloadExchanged {
        their_payload: Vec<u8>,
        device_id: String,
    },
    /// BLE proximity verified (challenge-response passed).
    BleProximityVerified,

    // --- Proximity events ---
    /// Proximity check completed with a confidence level.
    ProximityCheckCompleted { confidence: ProximityConfidence },
}

/// An exchange session managing the state of a contact exchange.
///
/// Stores the proximity verifier as `Box<dyn ProximityVerifier>`, eliminating
/// the need for generic type parameters and enabling VerifierChain to be used
/// directly without enum dispatch wrappers.
pub struct ExchangeSession {
    /// Current state
    state: ExchangeState,
    /// Transport mechanism (QR, NFC, BLE)
    transport: ExchangeTransport,
    /// Our identity
    identity: Identity,
    /// Our contact card to share
    our_card: ContactCard,
    /// Our X3DH keypair for this session (fresh ephemeral)
    our_x3dh: X3DHKeyPair,
    /// Proximity verifier (trait object — supports any verifier or chain).
    proximity: Box<dyn ProximityVerifier>,
    /// Proximity confidence result from the last proximity check.
    proximity_confidence: ProximityConfidence,
    /// When the session started
    started_at: Instant,
    /// Whether the session was interrupted
    interrupted: bool,
    /// Hashes of QR codes that have already been consumed (prevents reuse).
    used_qrs: HashSet<[u8; 32]>,
    /// The audio challenge extracted from the peer's QR code.
    /// Used for session-bound proximity verification (AU-3).
    their_audio_challenge: Option<[u8; 16]>,
    /// Our audio challenge from our QR code (for two-way verification).
    our_audio_challenge: Option<[u8; 16]>,
    /// The display name extracted from the peer's QR code.
    their_display_name: Option<String>,
    /// NFC handshake session (only populated for NFC transport).
    nfc_handshake: Option<NfcHandshakeSession>,
    /// BLE encrypted handshake session (only populated for BLE transport).
    ble_handshake: Option<BleHandshakeSession>,
    /// Our relay URL to include in QR code (for per-contact routing).
    our_relay_url: Option<String>,
    /// Our relay's Noise NK public key to include in QR code.
    our_relay_noise_pubkey: Option<[u8; 32]>,
    /// Their relay URL extracted from their QR code.
    their_relay_url: Option<String>,
    /// Their relay's Noise NK public key extracted from their QR code.
    their_relay_noise_pubkey: Option<[u8; 32]>,
    /// Optional exchange debug log. When enabled, captures timestamped
    /// events at each state transition for diagnostic analysis.
    debug_log: Option<ExchangeDebugLog>,
}

// Compile-time assertion: ExchangeSession must be Send + Sync because
// MobileExchangeSession wraps it in a Mutex for UniFFI cross-thread access.
// ProximityVerifier requires Send + Sync, so Box<dyn ProximityVerifier>
// satisfies this. If the bound is ever weakened, this will fail to compile.
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ExchangeSession>();
};

impl ExchangeSession {
    /// Creates a new QR exchange session.
    ///
    /// Both parties display QR codes with fresh ephemeral X25519 keys and
    /// scan each other's. This gives bidirectional identity verification
    /// and full forward secrecy (no identity-derived X3DH keys used).
    ///
    /// The `proximity` parameter accepts any `ProximityVerifier` implementation.
    /// The `'static` bound is required because the verifier is stored as a trait
    /// object (`Box<dyn ProximityVerifier>`). All current implementations are
    /// fully owned types and satisfy this bound.
    pub fn new_qr(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
    ) -> Self {
        // Fresh ephemeral keypair — NOT derived from identity
        let our_x3dh = X3DHKeyPair::generate();
        ExchangeSession {
            state: ExchangeState::Idle,
            transport: ExchangeTransport::Qr,
            identity,
            our_card,
            our_x3dh,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
        }
    }

    /// Test-only constructor that accepts a specific X3DH keypair for deterministic testing.
    #[cfg(any(test, feature = "testing"))]
    pub fn new_qr_with_x3dh(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
        our_x3dh: X3DHKeyPair,
    ) -> Self {
        ExchangeSession {
            state: ExchangeState::Idle,
            transport: ExchangeTransport::Qr,
            identity,
            our_card,
            our_x3dh,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
        }
    }

    /// Creates a new NFC active exchange session.
    ///
    /// A single NFC tap replaces both QR scan and proximity verification.
    /// Both sides use fresh ephemeral X25519 keys for full forward secrecy.
    /// The session starts in `AwaitingNfcTap` — ready to receive a tap event.
    pub fn new_nfc(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
    ) -> Self {
        let our_x3dh = X3DHKeyPair::generate();
        let display_name = our_card.display_name().to_string();
        let nfc_handshake = NfcHandshakeSession::new_initiator(&identity, display_name);
        ExchangeSession {
            state: ExchangeState::AwaitingNfcTap,
            transport: ExchangeTransport::Nfc,
            identity,
            our_card,
            our_x3dh,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: Some(nfc_handshake),
            ble_handshake: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
        }
    }

    /// Creates a new BLE exchange session.
    ///
    /// Uses GATT-based payload exchange with proximity verification.
    /// Both sides use fresh ephemeral X25519 keys for full forward secrecy.
    /// The session starts in `AwaitingBleConnection` — ready to receive a BLE event.
    pub fn new_ble(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
    ) -> Self {
        let our_x3dh = X3DHKeyPair::generate();
        let card = our_card.clone();
        let ble_card = BleCardPayload::new(
            *identity.signing_public_key(),
            card.display_name().to_string(),
            *our_x3dh.public_key(),
            card.fields()
                .iter()
                .map(|f| (f.label().to_string(), f.value().to_string()))
                .collect(),
            card.avatar().map(|a| a.to_vec()),
        );
        let ble_handshake = BleHandshakeSession::new_initiator(&identity, ble_card);
        ExchangeSession {
            state: ExchangeState::AwaitingBleConnection,
            transport: ExchangeTransport::Ble,
            identity,
            our_card,
            our_x3dh,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: Some(ble_handshake),
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &ExchangeState {
        &self.state
    }

    /// Returns the transport mechanism.
    pub fn transport(&self) -> ExchangeTransport {
        self.transport
    }

    /// Sets our relay URL to include in the QR code.
    /// Must be called before `process(StartQR)`.
    pub fn set_our_relay_url(&mut self, url: Option<String>) {
        self.our_relay_url = url;
    }

    /// Sets our relay's Noise NK public key to include in the QR code.
    /// Must be called before `process(StartQR)`.
    pub fn set_our_relay_noise_pubkey(&mut self, pubkey: Option<[u8; 32]>) {
        self.our_relay_noise_pubkey = pubkey;
    }

    /// Returns the QR code if in DisplayingQr or PeerScanned state.
    pub fn qr(&self) -> Option<&ExchangeQR> {
        match &self.state {
            ExchangeState::DisplayingQr { our_qr } => Some(our_qr),
            ExchangeState::PeerScanned { our_qr, .. } => Some(our_qr),
            _ => None,
        }
    }

    /// Returns the NFC handshake session (only for NFC transport).
    pub fn nfc_handshake(&self) -> Option<&NfcHandshakeSession> {
        self.nfc_handshake.as_ref()
    }

    /// Returns mutable access to the NFC handshake session.
    pub fn nfc_handshake_mut(&mut self) -> Option<&mut NfcHandshakeSession> {
        self.nfc_handshake.as_mut()
    }

    /// Returns the BLE encrypted handshake session (only for BLE transport).
    pub fn ble_handshake(&self) -> Option<&BleHandshakeSession> {
        self.ble_handshake.as_ref()
    }

    /// Returns mutable access to the BLE encrypted handshake session.
    pub fn ble_handshake_mut(&mut self) -> Option<&mut BleHandshakeSession> {
        self.ble_handshake.as_mut()
    }

    /// Returns the peer's audio challenge if one has been stored from their QR code.
    pub fn their_audio_challenge(&self) -> Option<&[u8; 16]> {
        self.their_audio_challenge.as_ref()
    }

    /// Returns the peer's display name if one has been extracted from their QR code.
    pub fn their_display_name(&self) -> Option<&str> {
        self.their_display_name.as_deref()
    }

    /// Returns the proximity confidence from the last verification.
    pub fn proximity_confidence(&self) -> ProximityConfidence {
        self.proximity_confidence
    }

    /// Returns the verification event log from the proximity verifier.
    ///
    /// Only populated when using a `VerifierChain`. Single verifiers
    /// return `None` (no event logging).
    pub fn proximity_event_log(&self) -> Option<super::VerifierEventLog> {
        self.proximity.verification_event_log()
    }

    /// Returns a reference to the proximity verifier (test-only).
    #[cfg(any(test, feature = "testing"))]
    pub fn proximity_verifier(&self) -> &dyn ProximityVerifier {
        &*self.proximity
    }

    /// Enable exchange debug logging. Records a `SessionStarted` event
    /// and captures timestamped events at each subsequent state transition.
    ///
    /// This is a production API — the platform debug panel calls it when
    /// the user activates debug mode. Not gated behind `cfg(test)`.
    ///
    /// Idempotent: calling on an already-enabled session is a no-op.
    pub fn enable_debug_log(&mut self) {
        if self.debug_log.is_some() {
            return;
        }
        let mut log = ExchangeDebugLog::new();
        log.push(ExchangeDebugEvent::SessionStarted {
            transport: Self::transport_label(self.transport).to_string(),
        });
        self.debug_log = Some(log);
    }

    /// Returns the exchange debug log, if enabled.
    pub fn exchange_debug_log(&self) -> Option<&ExchangeDebugLog> {
        self.debug_log.as_ref()
    }

    /// Push a debug event if logging is enabled.
    fn debug_event(&mut self, event: ExchangeDebugEvent) {
        if let Some(ref mut log) = self.debug_log {
            log.push(event);
        }
    }

    /// Lowercase label for a transport type (stable JSONL output).
    fn transport_label(t: ExchangeTransport) -> &'static str {
        match t {
            ExchangeTransport::Qr => "qr",
            ExchangeTransport::Nfc => "nfc",
            ExchangeTransport::Ble => "ble",
        }
    }

    /// Lowercase label for a confidence level (stable JSONL output).
    fn confidence_label(c: ProximityConfidence) -> &'static str {
        match c {
            ProximityConfidence::High => "high",
            ProximityConfidence::Medium => "medium",
            ProximityConfidence::Low => "low",
            ProximityConfidence::Unknown => "unknown",
        }
    }

    /// Checks whether a QR code hash has already been consumed.
    ///
    /// If the hash is new, it is recorded and `Ok(())` is returned.
    /// If the hash was already seen, returns `ExchangeError::QRAlreadyUsed`.
    pub fn check_qr_reuse(&mut self, qr_hash: &[u8; 32]) -> Result<(), ExchangeError> {
        if !self.used_qrs.insert(*qr_hash) {
            return Err(ExchangeError::QRAlreadyUsed);
        }
        Ok(())
    }

    /// Checks if the session has timed out.
    pub fn is_timed_out(&self) -> bool {
        self.started_at.elapsed() > SESSION_TIMEOUT
    }

    /// Checks if the session can be resumed (within timeout window).
    pub fn can_resume(&self) -> bool {
        self.interrupted && !self.is_timed_out()
    }

    /// Marks the session as interrupted.
    pub fn mark_interrupted(&mut self) {
        self.interrupted = true;
    }

    /// Processes an event and transitions the state machine.
    ///
    /// Returns `SessionTimeout` if the session has exceeded `SESSION_TIMEOUT` (#196).
    /// The `Fail` event is exempt so that callers can always cleanly terminate a session.
    pub fn apply(&mut self, event: ExchangeEvent) -> Result<(), ExchangeError> {
        // Enforce session timeout on all events except Fail (#196)
        if !matches!(event, ExchangeEvent::Fail(_)) && self.is_timed_out() {
            self.fail(ExchangeError::SessionTimeout);
            return Err(ExchangeError::SessionTimeout);
        }
        match event {
            // QR events
            ExchangeEvent::StartQR => self.handle_start_qr(),
            ExchangeEvent::ProcessQR(qr) => self.handle_process_qr(qr),
            ExchangeEvent::TheyScannedOurQR => self.handle_they_scanned_our_qr(),
            // Shared events
            ExchangeEvent::PerformKeyAgreement => self.handle_perform_key_agreement(),
            ExchangeEvent::CompleteExchange(card) => {
                self.handle_complete_exchange(card).map(|_| ())
            }
            ExchangeEvent::Fail(err) => {
                self.debug_event(ExchangeDebugEvent::ExchangeFailed {
                    error: err.to_string(),
                });
                self.fail(err);
                Ok(())
            }
            // NFC
            ExchangeEvent::NfcTapComplete { their_payload } => {
                self.handle_nfc_tap_complete(their_payload)
            }
            // BLE
            ExchangeEvent::StartBleExchange => self.handle_start_ble_exchange(),
            ExchangeEvent::BlePayloadExchanged {
                their_payload,
                device_id,
            } => self.handle_ble_payload_exchanged(their_payload, device_id),
            ExchangeEvent::BleProximityVerified => self.handle_ble_proximity_verified(),
            // Proximity
            ExchangeEvent::ProximityCheckCompleted { confidence } => {
                self.proximity_confidence = confidence;
                Ok(())
            }
        }
    }

    /// Runs a proximity check using the session's proximity verifier.
    ///
    /// Sets the proximity confidence based on the result:
    /// - Success -> verifier's confidence_level() (High, Medium, etc.)
    /// - NotSupported -> Unknown (device can't verify, not a failure)
    /// - Other errors (timeout, no response, hardware) -> Low
    pub fn run_proximity_check(&mut self) {
        use super::ProximityError;

        // AU-1: Use challenges from QR codes for session-bound verification.
        let their_challenge = self.their_audio_challenge.unwrap_or([0u8; 16]);
        let our_challenge = self.our_audio_challenge.unwrap_or([0u8; 16]);
        let timeout = Duration::from_secs(5);
        // Scanner (initiator) emits first; displayer (responder) listens first
        let is_initiator = self.their_audio_challenge.is_some();

        let confidence = match self.proximity.verify_proximity_two_way(
            &their_challenge,
            &our_challenge,
            timeout,
            is_initiator,
        ) {
            // AU-4: Use trait-based confidence level
            Ok(()) => self.proximity.confidence_level(),
            // AU-5: Device doesn't support proximity verification
            Err(ProximityError::NotSupported) => ProximityConfidence::Unknown,
            Err(_) => ProximityConfidence::Low,
        };
        self.proximity_confidence = confidence;
    }

    fn handle_perform_key_agreement(&mut self) -> Result<(), ExchangeError> {
        let (their_public_key, their_exchange_key) = match &self.state {
            ExchangeState::AwaitingKeyAgreement {
                their_public_key,
                their_exchange_key,
            } => (*their_public_key, *their_exchange_key),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Not in key agreement state".into(),
                ))
            }
        };

        // Symmetric DH: both sides have fresh ephemeral keys.
        // DH(our_secret × their_exchange_key) — both sides compute the same shared secret.
        // HKDF info binds all four public keys into the derivation (transcript binding),
        // preventing identity misbinding attacks. Keys are sorted lexicographically so
        // both sides compute identical info regardless of who is "Alice" vs "Bob".
        let shared_bytes = self.our_x3dh.diffie_hellman(&their_exchange_key)?;
        let our_id = self.identity.signing_public_key();
        let our_eph = self.our_x3dh.public_key();
        let (id_lo, id_hi) = if our_id < &their_public_key {
            (our_id.as_slice(), their_public_key.as_slice())
        } else {
            (their_public_key.as_slice(), our_id.as_slice())
        };
        let (eph_lo, eph_hi) = if our_eph < &their_exchange_key {
            (our_eph.as_slice(), their_exchange_key.as_slice())
        } else {
            (their_exchange_key.as_slice(), our_eph.as_slice())
        };
        let mut info = b"vauchi-x3dh-symmetric-v2".to_vec();
        info.extend_from_slice(id_lo);
        info.extend_from_slice(id_hi);
        info.extend_from_slice(eph_lo);
        info.extend_from_slice(eph_hi);
        let derived = HKDF::derive_key(None, &*shared_bytes, &info);
        let shared_key = crate::crypto::SymmetricKey::from_bytes(*derived);

        self.state = ExchangeState::AwaitingCardExchange {
            their_public_key,
            shared_key,
        };

        self.debug_event(ExchangeDebugEvent::KeyAgreementCompleted);

        // AU-2: Auto-invoke proximity check after key agreement.
        // NFC is exempt: the physical tap IS the proximity proof — running a
        // separate verifier is redundant and could fail on devices without
        // audio hardware, causing a false negative.
        if self.transport == ExchangeTransport::Nfc {
            self.proximity_confidence = ProximityConfidence::High;
        } else {
            self.debug_event(ExchangeDebugEvent::ProximityCheckStarted {
                method: Self::transport_label(self.transport).to_string(),
            });
            self.run_proximity_check();
            self.debug_event(ExchangeDebugEvent::ProximityCheckCompleted {
                confidence: Self::confidence_label(self.proximity_confidence).to_string(),
            });
        }

        Ok(())
    }

    fn handle_complete_exchange(
        &mut self,
        their_card: ContactCard,
    ) -> Result<Contact, ExchangeError> {
        let (their_public_key, shared_key) =
            match std::mem::replace(&mut self.state, ExchangeState::Idle) {
                ExchangeState::AwaitingCardExchange {
                    their_public_key,
                    shared_key,
                } => (their_public_key, shared_key),
                other => {
                    self.state = other;
                    return Err(ExchangeError::InvalidState(
                        "Not in card exchange state".into(),
                    ));
                }
            };

        let mut contact = Contact::from_exchange_full(
            their_public_key,
            their_card,
            shared_key,
            self.proximity_confidence,
            self.transport,
        );

        // Set relay metadata learned from their QR code
        contact.set_relay_url(self.their_relay_url.take());
        contact.set_relay_noise_pubkey(self.their_relay_noise_pubkey.take());

        self.state = ExchangeState::Complete {
            contact: contact.clone(),
        };
        self.debug_event(ExchangeDebugEvent::ExchangeCompleted);

        Ok(contact)
    }

    // ---- QR handlers ----

    fn handle_start_qr(&mut self) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Qr {
            return Err(ExchangeError::InvalidState(
                "StartQR requires Qr transport".into(),
            ));
        }
        if !matches!(self.state, ExchangeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Can only start QR from Idle state".into(),
            ));
        }

        let our_qr = ExchangeQR::generate_with_relay(
            &self.identity,
            &self.our_x3dh,
            self.our_relay_url.clone(),
            self.our_relay_noise_pubkey,
        );
        self.our_audio_challenge = Some(*our_qr.audio_challenge());
        self.state = ExchangeState::DisplayingQr { our_qr };
        self.debug_event(ExchangeDebugEvent::QrGenerated);
        Ok(())
    }

    fn handle_process_qr(&mut self, qr: ExchangeQR) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Qr {
            return Err(ExchangeError::InvalidState(
                "ProcessQR requires Qr transport".into(),
            ));
        }

        let our_qr = match &self.state {
            ExchangeState::DisplayingQr { our_qr } => our_qr.clone(),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Can only scan their QR from DisplayingQr state".into(),
                ));
            }
        };

        // Verify their QR
        if qr.is_expired() {
            return Err(ExchangeError::QRExpired);
        }
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *qr.public_key();
        let their_exchange_key = *qr.exchange_key();

        // Self-exchange check
        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        // AU-3: Store their audio challenge for session-bound proximity verification
        self.their_audio_challenge = Some(*qr.audio_challenge());
        // Store their display name from the QR code
        self.their_display_name = Some(qr.display_name().to_string());
        // Store their relay metadata for per-contact routing
        self.their_relay_url = qr.relay_url().map(String::from);
        self.their_relay_noise_pubkey = qr.relay_noise_pubkey().copied();

        self.state = ExchangeState::PeerScanned {
            our_qr,
            their_public_key,
            their_exchange_key,
        };
        self.debug_event(ExchangeDebugEvent::QrScanned);

        Ok(())
    }

    fn handle_they_scanned_our_qr(&mut self) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Qr {
            return Err(ExchangeError::InvalidState(
                "TheyScannedOurQR requires Qr transport".into(),
            ));
        }

        let (their_public_key, their_exchange_key) = match &self.state {
            ExchangeState::PeerScanned {
                their_public_key,
                their_exchange_key,
                ..
            } => (*their_public_key, *their_exchange_key),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Can only confirm their scan from PeerScanned state".into(),
                ));
            }
        };

        // Transition to shared key agreement path
        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
    }

    // ---- NFC handlers ----

    fn handle_nfc_tap_complete(&mut self, their_payload: Vec<u8>) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Nfc {
            return Err(ExchangeError::InvalidState(
                "NfcTapComplete requires Nfc transport".into(),
            ));
        }
        if !matches!(self.state, ExchangeState::AwaitingNfcTap) {
            return Err(ExchangeError::InvalidState(
                "Can only complete NFC tap from AwaitingNfcTap state".into(),
            ));
        }

        // Parse their NFC payload to extract keys
        let parsed = super::nfc_active::ExchangeNfc::from_bytes(&their_payload)?;

        if parsed.is_expired() {
            return Err(ExchangeError::NfcExpired);
        }
        if !parsed.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *parsed.identity_key();
        let their_exchange_key = *parsed.exchange_key();

        // Self-exchange check
        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
    }

    // ---- BLE handlers ----

    fn handle_start_ble_exchange(&mut self) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Ble {
            return Err(ExchangeError::InvalidState(
                "StartBleExchange requires Ble transport".into(),
            ));
        }
        if !matches!(
            self.state,
            ExchangeState::Idle | ExchangeState::AwaitingBleConnection
        ) {
            return Err(ExchangeError::InvalidState(
                "Can only start BLE exchange from Idle or AwaitingBleConnection state".into(),
            ));
        }

        self.state = ExchangeState::AwaitingBleConnection;
        Ok(())
    }

    fn handle_ble_payload_exchanged(
        &mut self,
        their_payload: Vec<u8>,
        device_id: String,
    ) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Ble {
            return Err(ExchangeError::InvalidState(
                "BlePayloadExchanged requires Ble transport".into(),
            ));
        }
        if !matches!(self.state, ExchangeState::AwaitingBleConnection) {
            return Err(ExchangeError::InvalidState(
                "Can only exchange BLE payload from AwaitingBleConnection state".into(),
            ));
        }

        let parsed = super::ble::ExchangeBle::from_bytes(&their_payload)?;

        if parsed.is_expired() {
            return Err(ExchangeError::BleExpired);
        }
        if !parsed.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *parsed.identity_key();
        let their_exchange_key = *parsed.exchange_key();

        // Self-exchange check
        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        self.state = ExchangeState::AwaitingBleVerification {
            their_public_key,
            their_exchange_key,
            device_id,
        };
        Ok(())
    }

    fn handle_ble_proximity_verified(&mut self) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Ble {
            return Err(ExchangeError::InvalidState(
                "BleProximityVerified requires Ble transport".into(),
            ));
        }

        let (their_public_key, their_exchange_key) = match &self.state {
            ExchangeState::AwaitingBleVerification {
                their_public_key,
                their_exchange_key,
                ..
            } => (*their_public_key, *their_exchange_key),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Can only verify BLE proximity from AwaitingBleVerification state".into(),
                ));
            }
        };

        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
    }

    /// Returns our card (for sending to the other party).
    pub fn our_card(&self) -> &ContactCard {
        &self.our_card
    }

    /// Returns our X3DH public key (for generating transport payloads).
    pub fn our_exchange_public_key(&self) -> &[u8; 32] {
        self.our_x3dh.public_key()
    }

    /// Fails the session with an error.
    pub fn fail(&mut self, error: ExchangeError) {
        self.state = ExchangeState::Failed { error };
    }

    /// Checks if a contact already exists in the given list.
    ///
    /// Returns the existing contact if found (matched by public key).
    pub fn check_duplicate<'a>(&self, contacts: &'a [Contact]) -> Option<&'a Contact> {
        let their_key = match &self.state {
            ExchangeState::PeerScanned {
                their_public_key, ..
            }
            | ExchangeState::AwaitingKeyAgreement {
                their_public_key, ..
            }
            | ExchangeState::AwaitingCardExchange {
                their_public_key, ..
            }
            | ExchangeState::AwaitingBleVerification {
                their_public_key, ..
            } => Some(their_public_key),
            _ => None,
        };

        their_key.and_then(|key| contacts.iter().find(|c| c.public_key() == key))
    }
}

/// Platform-specific callbacks for pre-exchange checks.
///
/// Mobile and desktop platforms can implement this trait to perform
/// device-level checks (battery, storage) before starting an exchange.
pub trait ExchangePlatformCallbacks: Send + Sync {
    /// Check whether the device battery level is sufficient for an exchange.
    fn check_battery_level(&self) -> Result<(), ExchangeError>;

    /// Check whether the device has enough free storage for an exchange.
    fn check_storage_available(&self) -> Result<(), ExchangeError>;
}

/// Default no-op implementation of platform callbacks.
///
/// Always succeeds, suitable for platforms where battery/storage
/// checks are not applicable (e.g., CLI, tests).
pub struct DefaultPlatformCallbacks;

impl ExchangePlatformCallbacks for DefaultPlatformCallbacks {
    fn check_battery_level(&self) -> Result<(), ExchangeError> {
        Ok(())
    }

    fn check_storage_available(&self) -> Result<(), ExchangeError> {
        Ok(())
    }
}

/// Action to take when a duplicate contact is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuplicateAction {
    /// Update the existing contact with new information
    Update,
    /// Keep the existing contact unchanged
    Keep,
    /// Cancel the exchange
    Cancel,
}

// Add InvalidState variant to ExchangeError
impl From<&str> for ExchangeError {
    fn from(s: &str) -> Self {
        ExchangeError::InvalidState(s.to_string())
    }
}
