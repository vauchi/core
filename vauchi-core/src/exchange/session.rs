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
use super::command::{ExchangeCommand, ExchangeHardwareEvent};
use super::nfc_handshake::NfcHandshakeSession;
use super::trust_metrics::TrustMetrics;
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
#[non_exhaustive]
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
    Complete { contact: Box<Contact> },
    /// Exchange failed
    Failed { error: ExchangeError },
}

/// Events that drive the exchange state machine.
#[derive(Debug)]
#[non_exhaustive]
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
    /// Device hardware capabilities for transport fallback decisions.
    device_capabilities: Option<super::capability::types::DeviceCapabilities>,
    /// Whether we initiated the BLE connection (scanner role).
    /// Set to `true` on `BleDeviceDiscovered`, determines who sends KeyOffer first.
    ble_is_initiator: bool,
    /// Buffered BLE handshake data (KeyAck or commitment) awaiting card data.
    ble_pending_handshake: Option<Vec<u8>>,
    /// Buffered BLE encrypted card data awaiting handshake data.
    ble_pending_card: Option<Vec<u8>>,
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
    /// Pending commands to be sent to the frontend (ADR-031).
    /// Populated by `apply_hardware_event()` and drained by `drain_commands()`.
    pending_commands: Vec<ExchangeCommand>,
    /// Our reciprocity confirmation token (derived in key agreement, zeroized on drop).
    our_confirmation_token: Option<zeroize::Zeroizing<[u8; 32]>>,
    /// Token we expect from the peer (derived in key agreement, zeroized on drop).
    expected_their_token: Option<zeroize::Zeroizing<[u8; 32]>>,
    /// Confirmation escrow gate hash (derived in key agreement).
    confirmation_gate_hash: Option<String>,
    /// Our confirmation escrow slot hash.
    confirmation_our_slot: Option<String>,
    /// Their confirmation escrow slot hash.
    confirmation_their_slot: Option<String>,
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
            device_capabilities: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
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
            device_capabilities: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
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
            device_capabilities: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
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
            device_capabilities: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            our_relay_noise_pubkey: None,
            their_relay_url: None,
            their_relay_noise_pubkey: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
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

    /// Sets device capabilities for transport fallback decisions.
    ///
    /// When a transport reports `HardwareUnavailable`, the session uses these
    /// capabilities to determine if a fallback transport is available (e.g.,
    /// BLE → QR if `has_camera` is true).
    pub fn set_device_capabilities(&mut self, caps: super::capability::types::DeviceCapabilities) {
        self.device_capabilities = Some(caps);
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

    /// Builds TrustMetrics from the current session state.
    ///
    /// Called internally when creating a contact after successful exchange.
    fn build_trust_metrics(&self) -> TrustMetrics {
        let log = self.proximity_event_log().unwrap_or_default();
        let method = log.final_method();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
        TrustMetrics::new(
            self.transport,
            self.proximity_confidence,
            method,
            log,
            timestamp,
        )
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
            ExchangeTransport::Usb => "usb",
            ExchangeTransport::Audio => "audio",
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

    // ── ADR-031: Command/event protocol ────────────────────────────────

    /// Returns and clears all pending commands.
    ///
    /// Frontends call this after `apply()` or `apply_hardware_event()` to
    /// get the list of hardware actions they need to perform.
    pub fn drain_commands(&mut self) -> Vec<ExchangeCommand> {
        std::mem::take(&mut self.pending_commands)
    }

    /// Our reciprocity confirmation token, if key agreement has been performed.
    pub fn our_confirmation_token(&self) -> Option<&[u8; 32]> {
        self.our_confirmation_token.as_deref()
    }

    /// The token we expect from the peer, if key agreement has been performed.
    pub fn expected_their_token(&self) -> Option<&[u8; 32]> {
        self.expected_their_token.as_deref()
    }

    /// Confirmation escrow identifiers (gate, our_slot, their_slot).
    pub fn confirmation_escrow(&self) -> Option<(&str, &str, &str)> {
        match (
            &self.confirmation_gate_hash,
            &self.confirmation_our_slot,
            &self.confirmation_their_slot,
        ) {
            (Some(g), Some(o), Some(t)) => Some((g, o, t)),
            _ => None,
        }
    }

    /// Queues a command for the frontend.
    fn emit_command(&mut self, cmd: ExchangeCommand) {
        self.pending_commands.push(cmd);
    }

    /// Processes a hardware event from the frontend and advances the state machine.
    ///
    /// This is the ADR-031 entry point. Frontends report hardware results
    /// (QR scanned, BLE data received, etc.) and this method:
    /// 1. Converts the event to an internal `ExchangeEvent`
    /// 2. Calls `apply()` to advance the state machine
    /// 3. Emits response commands based on the new state
    ///
    /// After calling this, use `drain_commands()` to get outgoing commands.
    pub fn apply_hardware_event(
        &mut self,
        event: ExchangeHardwareEvent,
    ) -> Result<(), ExchangeError> {
        match event {
            ExchangeHardwareEvent::QrScanned { data } => {
                let qr = ExchangeQR::from_data_string(&data)?;
                self.apply(ExchangeEvent::ProcessQR(qr))
            }
            ExchangeHardwareEvent::NfcDataReceived { data } => {
                let result = self.apply(ExchangeEvent::NfcTapComplete {
                    their_payload: data,
                });
                // Deactivate NFC interface after tap is processed
                if result.is_ok() {
                    self.emit_command(ExchangeCommand::NfcDeactivate);
                }
                result
            }
            ExchangeHardwareEvent::BleConnected { device_id } => {
                self.handle_ble_connected(device_id)
            }
            ExchangeHardwareEvent::BleCharacteristicRead { uuid, data }
            | ExchangeHardwareEvent::BleCharacteristicNotified { uuid, data } => {
                self.handle_ble_characteristic_data(uuid, data)
            }
            ExchangeHardwareEvent::AudioResponseReceived { .. } => {
                // Audio proximity response — trigger proximity check.
                // The actual verification is done by the ProximityVerifier.
                Ok(())
            }
            ExchangeHardwareEvent::BleDeviceDiscovered { id, .. } => {
                // Discovered a peer — stop scanning (battery), connect.
                self.ble_is_initiator = true;
                self.emit_command(ExchangeCommand::BleStopScanning);
                self.emit_command(ExchangeCommand::BleConnect { device_id: id });
                Ok(())
            }
            ExchangeHardwareEvent::BleDisconnected { reason } => {
                if matches!(self.state, ExchangeState::AwaitingBleConnection) {
                    self.apply(ExchangeEvent::Fail(ExchangeError::BleConnectionLost))?;
                }
                self.debug_event(ExchangeDebugEvent::ExchangeFailed {
                    error: format!("BLE disconnected: {}", reason),
                });
                Ok(())
            }
            ExchangeHardwareEvent::HardwareError { transport, error } => {
                self.apply(ExchangeEvent::Fail(ExchangeError::HardwareFailure {
                    transport,
                    error,
                }))
            }
            ExchangeHardwareEvent::HardwareUnavailable { transport } => {
                self.debug_event(ExchangeDebugEvent::ExchangeFailed {
                    error: format!("{} hardware unavailable", transport),
                });
                // Attempt transport fallback based on device capabilities.
                self.attempt_transport_fallback(&transport);
                Ok(())
            }
            // New hardware event variants — not yet wired into the session state machine.
            // Frontends may send these; they are acknowledged without state change until
            // the corresponding session logic is implemented.
            ExchangeHardwareEvent::AccelerometerData { .. }
            | ExchangeHardwareEvent::ImpactDetected { .. }
            | ExchangeHardwareEvent::RelayEscrowReady { .. }
            | ExchangeHardwareEvent::RelayEscrowBlobReceived { .. }
            | ExchangeHardwareEvent::RelayEscrowFailed { .. }
            | ExchangeHardwareEvent::LinkShared
            | ExchangeHardwareEvent::LinkOpened { .. } => Ok(()),
        }
    }

    /// Emits initial commands for the current transport type.
    ///
    /// Call this after creating a new session to get the first set of
    /// hardware commands (e.g., `QrDisplay` for QR sessions, `BleStartScanning`
    /// for BLE sessions). Use `drain_commands()` to retrieve them.
    pub fn emit_initial_commands(&mut self) {
        match (&self.state, self.transport) {
            (ExchangeState::DisplayingQr { our_qr }, ExchangeTransport::Qr) => {
                self.pending_commands.push(ExchangeCommand::QrDisplay {
                    data: our_qr.to_data_string(),
                });
            }
            (ExchangeState::AwaitingNfcTap, ExchangeTransport::Nfc) => {
                // Generate our NFC key offer payload for the frontend to present.
                // The frontend activates the NFC interface with this data, and when
                // the peer taps, sends their data back as NfcDataReceived.
                let payload = if let Some(ref mut hs) = self.nfc_handshake {
                    hs.create_key_offer(&self.identity).unwrap_or_default()
                } else {
                    // Fallback: generate ExchangeNfc payload directly
                    let nfc =
                        super::nfc_active::ExchangeNfc::generate(&self.identity, &self.our_x3dh);
                    nfc.to_bytes().to_vec()
                };
                self.pending_commands
                    .push(ExchangeCommand::NfcActivate { payload });
            }
            (ExchangeState::AwaitingBleConnection, ExchangeTransport::Ble) => {
                self.pending_commands
                    .push(ExchangeCommand::BleStartScanning {
                        service_uuid: super::VAUCHI_BLE_SERVICE_UUID.to_string(),
                    });
                self.pending_commands
                    .push(ExchangeCommand::BleStartAdvertising {
                        service_uuid: super::VAUCHI_BLE_SERVICE_UUID.to_string(),
                        payload: Vec::new(),
                    });
            }
            _ => {}
        }
    }

    // ── ADR-031: Audio proximity commands ────────────────────────────────

    /// Emits audio commands for async proximity verification after key agreement.
    ///
    /// If audio challenges are available (from QR codes), emits
    /// `AudioEmitChallenge` and/or `AudioListenForResponse` commands.
    /// The frontend handles audio I/O via CpalAudioBackend (desktop) or
    /// PlatformAudioBackend (mobile) and reports `AudioResponseReceived`.
    ///
    /// If no challenges are available, falls back to the synchronous
    /// ProximityVerifier (e.g., ManualConfirmationVerifier).
    fn emit_proximity_commands(&mut self) {
        // Always run the synchronous verifier first — it provides a baseline
        // confidence and populates verification event logs for diagnostics.
        // Then emit audio commands so ADR-031 frontends can perform
        // hardware-backed verification and upgrade the confidence to High.
        self.debug_event(ExchangeDebugEvent::ProximityCheckStarted {
            method: Self::transport_label(self.transport).to_string(),
        });
        self.run_proximity_check();
        self.debug_event(ExchangeDebugEvent::ProximityCheckCompleted {
            confidence: Self::confidence_label(self.proximity_confidence).to_string(),
        });

        // ADR-031: Also emit audio commands when QR challenges are available.
        // Frontends that support ultrasonic verification handle these and
        // report AudioResponseReceived to upgrade confidence. Frontends that
        // don't support audio simply ignore them (confidence stays at baseline).
        if let Some(their) = self.their_audio_challenge {
            let is_initiator = self.their_audio_challenge.is_some();
            if is_initiator {
                self.emit_command(ExchangeCommand::AudioEmitChallenge {
                    data: their.to_vec(),
                });
                self.emit_command(ExchangeCommand::AudioListenForResponse { timeout_ms: 5000 });
            } else {
                self.emit_command(ExchangeCommand::AudioListenForResponse { timeout_ms: 5000 });
                self.emit_command(ExchangeCommand::AudioEmitChallenge {
                    data: their.to_vec(),
                });
            }
        }
    }

    // ── ADR-031: Transport fallback ──────────────────────────────────────

    /// Attempts to fall back to an alternative transport when the current one
    /// reports unavailable. Checks `device_capabilities` to find a supported
    /// fallback and emits initial commands for that transport.
    ///
    /// Fallback priority: BLE/NFC → QR (camera required).
    /// QR is the universal fallback since it requires only a camera.
    fn attempt_transport_fallback(&mut self, _failed_transport: &str) {
        let caps = match &self.device_capabilities {
            Some(c) => c.clone(),
            None => return, // No capabilities set — can't determine fallback
        };

        // Only fall back if we haven't progressed past the initial transport state
        let can_fallback = matches!(
            self.state,
            ExchangeState::AwaitingBleConnection | ExchangeState::AwaitingNfcTap
        );
        if !can_fallback {
            return;
        }

        // Try QR fallback (requires camera)
        if caps.has_camera {
            // Switch to QR transport
            self.transport = ExchangeTransport::Qr;
            self.state = ExchangeState::Idle;
            // Start QR session — generates ephemeral keys and QR code
            if self.apply(ExchangeEvent::StartQR).is_ok() {
                self.emit_initial_commands();
            }
        }
    }

    // ── ADR-031: BLE command/event handlers ─────────────────────────────

    /// Handles a BLE connection event.
    ///
    /// If we're the initiator (saw `BleDeviceDiscovered` first), creates a
    /// `KeyOffer` and emits a `BleWriteCharacteristic` command to send it.
    /// Responders do nothing here — they wait for the KeyOffer to arrive.
    fn handle_ble_connected(&mut self, _device_id: String) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Ble {
            return Ok(());
        }
        if !self.ble_is_initiator {
            return Ok(()); // Responder waits for KeyOffer
        }
        let hs = match self.ble_handshake.as_mut() {
            Some(hs) => hs,
            None => return Ok(()),
        };
        let key_offer = hs.create_key_offer()?;
        self.emit_command(ExchangeCommand::BleWriteCharacteristic {
            uuid: super::CHAR_HANDSHAKE_WRITE.to_string(),
            data: key_offer,
        });
        Ok(())
    }

    /// Routes BLE characteristic data to the appropriate handshake phase.
    ///
    /// BLE handshake data arrives on two characteristics:
    /// - `CHAR_HANDSHAKE_NOTIFY`: KeyAck (Phase 2), reveal (Phase 4)
    /// - `CHAR_DATA_NOTIFY`: encrypted card data
    ///
    /// Phase 2 requires both a KeyAck and encrypted card. These may arrive
    /// in either order, so we buffer whichever comes first and process when
    /// both are available.
    fn handle_ble_characteristic_data(
        &mut self,
        uuid: String,
        data: Vec<u8>,
    ) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Ble {
            // Non-BLE session — fall back to legacy single-event handling
            return self.apply(ExchangeEvent::BlePayloadExchanged {
                their_payload: data,
                device_id: String::new(),
            });
        }

        let hs = match self.ble_handshake.as_ref() {
            Some(hs) => hs,
            None => return Ok(()),
        };

        // Route based on handshake state and characteristic UUID
        use super::ble_handshake::BleHandshakeState;
        match hs.state() {
            BleHandshakeState::KeyOfferSent { .. } => {
                // Initiator: waiting for KeyAck + encrypted card (Phase 2)
                self.buffer_and_process_phase2(uuid, data)
            }
            BleHandshakeState::AwaitingPayload { .. } => {
                // Responder: waiting for commitment + their encrypted card (Phase 3)
                self.buffer_and_process_phase3(uuid, data)
            }
            BleHandshakeState::PayloadsExchanged { .. } => {
                // Initiator: waiting for reveal (Phase 4)
                if uuid == super::CHAR_HANDSHAKE_NOTIFY {
                    self.process_ble_phase4(data)
                } else {
                    Ok(())
                }
            }
            _ => Ok(()),
        }
    }

    /// Buffers Phase 2 data (KeyAck + encrypted card) and processes when both arrive.
    fn buffer_and_process_phase2(
        &mut self,
        uuid: String,
        data: Vec<u8>,
    ) -> Result<(), ExchangeError> {
        if uuid == super::CHAR_HANDSHAKE_NOTIFY {
            self.ble_pending_handshake = Some(data);
        } else if uuid == super::CHAR_DATA_NOTIFY {
            self.ble_pending_card = Some(data);
        }

        // Process when both arrive
        if self.ble_pending_handshake.is_some() && self.ble_pending_card.is_some() {
            let key_ack = self.ble_pending_handshake.take().expect("checked Some");
            let their_card = self.ble_pending_card.take().expect("checked Some");
            let hs = self.ble_handshake.as_mut().ok_or_else(|| {
                ExchangeError::InvalidState("BLE handshake not initialized".into())
            })?;
            let (our_commitment, our_encrypted_card) = hs.process_key_ack(&key_ack, &their_card)?;

            self.emit_command(ExchangeCommand::BleWriteCharacteristic {
                uuid: super::CHAR_HANDSHAKE_WRITE.to_string(),
                data: our_commitment,
            });
            self.emit_command(ExchangeCommand::BleWriteCharacteristic {
                uuid: super::CHAR_DATA_WRITE.to_string(),
                data: our_encrypted_card,
            });
        }
        Ok(())
    }

    /// Buffers Phase 3 data (commitment + their encrypted card) and processes when both arrive.
    fn buffer_and_process_phase3(
        &mut self,
        uuid: String,
        data: Vec<u8>,
    ) -> Result<(), ExchangeError> {
        if uuid == super::CHAR_HANDSHAKE_WRITE {
            self.ble_pending_handshake = Some(data);
        } else if uuid == super::CHAR_DATA_WRITE {
            self.ble_pending_card = Some(data);
        }

        // Process when both arrive (guard without consuming)
        if self.ble_pending_handshake.is_some() && self.ble_pending_card.is_some() {
            let their_commitment = self.ble_pending_handshake.take().expect("checked Some");
            let their_card = self.ble_pending_card.take().expect("checked Some");
            let hs = self.ble_handshake.as_mut().ok_or_else(|| {
                ExchangeError::InvalidState("BLE handshake not initialized".into())
            })?;
            let reveal = hs.process_committed_payload(&their_commitment, &their_card)?;

            // Send reveal back
            self.emit_command(ExchangeCommand::BleWriteCharacteristic {
                uuid: super::CHAR_HANDSHAKE_NOTIFY.to_string(),
                data: reveal,
            });
        }
        Ok(())
    }

    /// Processes Phase 4: verify reveal and complete the exchange.
    fn process_ble_phase4(&mut self, reveal: Vec<u8>) -> Result<(), ExchangeError> {
        let hs = self
            .ble_handshake
            .as_mut()
            .ok_or_else(|| ExchangeError::InvalidState("No BLE handshake session".into()))?;

        let result = hs.complete_exchange(&reveal)?;

        // Build a ContactCard from the BLE payload fields
        let remote = &result.remote_card;
        let mut their_card = ContactCard::new(&remote.display_name);
        for (label, value) in &remote.fields {
            // Ignore field-count errors — BLE payload is already validated
            let _ = their_card.add_field(crate::contact_card::ContactField::new(
                crate::contact_card::FieldType::Custom,
                label,
                value,
            ));
        }
        if let Some(ref avatar) = remote.avatar {
            let _ = their_card.set_avatar(avatar.clone());
        }

        // Derive a relay-use shared key from both parties' identity keys.
        // This is deterministic — both sides compute the same key. Used for
        // relay message encryption, NOT for BLE session encryption (that's
        // handled by BleHandshakeSession's ephemeral DH).
        let our_id = self.identity.signing_public_key();
        let (id_lo, id_hi) = if our_id < &remote.identity_key {
            (our_id.as_slice(), remote.identity_key.as_slice())
        } else {
            (remote.identity_key.as_slice(), our_id.as_slice())
        };
        let mut relay_info = b"vauchi-ble-relay-key-v1".to_vec();
        relay_info.extend_from_slice(id_lo);
        relay_info.extend_from_slice(id_hi);
        let dh_bytes = self.our_x3dh.diffie_hellman(&remote.exchange_key)?;
        let relay_derived = HKDF::derive_key(None, &*dh_bytes, &relay_info);
        let relay_key = crate::crypto::SymmetricKey::from_bytes(*relay_derived);

        let mut contact = Contact::from_exchange_full(
            remote.identity_key,
            their_card,
            relay_key,
            self.proximity_confidence,
            self.transport,
        );
        contact.set_relay_url(self.their_relay_url.take());
        contact.set_relay_noise_pubkey(self.their_relay_noise_pubkey.take());

        // Record trust metrics from exchange signals
        contact.set_trust_metrics(Some(self.build_trust_metrics()));

        self.state = ExchangeState::Complete {
            contact: Box::new(contact.clone()),
        };
        self.debug_event(ExchangeDebugEvent::ExchangeCompleted);
        Ok(())
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
                ));
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

        // Derive reciprocity confirmation tokens (design spec §2).
        // Asymmetric: each side's token binds their own identity key.
        let our_confirm_info = [b"vauchi-reciprocity-confirm-v1".as_slice(), our_id].concat();
        let their_confirm_info = [
            b"vauchi-reciprocity-confirm-v1".as_slice(),
            their_public_key.as_slice(),
        ]
        .concat();
        let our_confirm = HKDF::derive_key(None, &*shared_bytes, &our_confirm_info);
        let their_confirm = HKDF::derive_key(None, &*shared_bytes, &their_confirm_info);
        self.our_confirmation_token = Some(our_confirm);
        self.expected_their_token = Some(their_confirm);

        // Derive confirmation escrow keys (design spec §3.5).
        // Role: smaller identity key = Initiator.
        let escrow_role = if our_id.as_slice() < their_public_key.as_slice() {
            super::escrow::EscrowRole::Initiator
        } else {
            super::escrow::EscrowRole::Responder
        };
        let confirm_escrow =
            super::confirmation_escrow::ConfirmationEscrowKeys::derive(&*shared_bytes, escrow_role);
        self.confirmation_gate_hash = Some(confirm_escrow.gate_hash);
        self.confirmation_our_slot = Some(confirm_escrow.our_slot);
        self.confirmation_their_slot = Some(confirm_escrow.their_slot);

        self.state = ExchangeState::AwaitingCardExchange {
            their_public_key,
            shared_key,
        };

        self.debug_event(ExchangeDebugEvent::KeyAgreementCompleted);

        // AU-2: Auto-invoke proximity check after key agreement.
        // NFC is exempt: the physical tap IS the proximity proof.
        if self.transport == ExchangeTransport::Nfc {
            self.proximity_confidence = ProximityConfidence::High;
        } else {
            // ADR-031: Emit audio commands for async proximity verification.
            // The frontend handles audio I/O and reports AudioResponseReceived.
            // If no audio challenges are available (no QR scanned), fall back
            // to the synchronous verifier (ManualConfirmation etc.).
            self.emit_proximity_commands();
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

        // Record trust metrics from exchange signals
        contact.set_trust_metrics(Some(self.build_trust_metrics()));

        self.state = ExchangeState::Complete {
            contact: Box::new(contact.clone()),
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

        their_key.and_then(|key| contacts.iter().find(|c| c.public_key() == Some(key)))
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
#[non_exhaustive]
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
