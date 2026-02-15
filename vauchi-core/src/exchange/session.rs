// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Session State Machine
//!
//! Manages the state of a contact exchange from QR generation through
//! key agreement and card exchange.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::{ExchangeError, ExchangeQR, ProximityVerifier, X3DHKeyPair};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::kdf::HKDF;
use crate::identity::Identity;

/// Session timeout duration (60 seconds for resumption).
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

/// Mode of the exchange session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExchangeMode {
    /// Both parties exchange contact cards (default).
    #[default]
    Mutual,
    /// Only the initiator sends their card; the responder does not share.
    ShareOnly,
}

/// Transport mechanism used for this exchange session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExchangeTransport {
    /// QR exchange: both sides display and scan QR codes.
    /// Both use fresh ephemeral X25519 keys for full forward secrecy.
    #[default]
    Qr,
    /// NFC Active (phone-to-phone tap): single tap replaces scan + proximity.
    /// Fresh ephemeral X25519 keys on both sides.
    Nfc,
    /// BLE exchange: GATT-based payload exchange with proximity verification.
    /// Fresh ephemeral X25519 keys on both sides.
    Ble,
}

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
}

/// An exchange session managing the state of a contact exchange.
pub struct ExchangeSession<P: ProximityVerifier> {
    /// Current state
    state: ExchangeState,
    /// Exchange mode (Mutual or ShareOnly)
    mode: ExchangeMode,
    /// Transport mechanism (QR, NFC, BLE)
    transport: ExchangeTransport,
    /// Our identity
    identity: Identity,
    /// Our contact card to share
    our_card: ContactCard,
    /// Our X3DH keypair for this session (fresh ephemeral)
    our_x3dh: X3DHKeyPair,
    /// Proximity verifier (used by NFC/BLE flows, not QR)
    #[allow(dead_code)]
    proximity: P,
    /// When the session started
    started_at: Instant,
    /// Whether the session was interrupted
    interrupted: bool,
    /// Hashes of QR codes that have already been consumed (prevents reuse).
    used_qrs: HashSet<[u8; 32]>,
}

impl<P: ProximityVerifier> ExchangeSession<P> {
    /// Creates a new QR exchange session.
    ///
    /// Both parties display QR codes with fresh ephemeral X25519 keys and
    /// scan each other's. This gives bidirectional identity verification
    /// and full forward secrecy (no identity-derived X3DH keys used).
    pub fn new_qr(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        // Fresh ephemeral keypair — NOT derived from identity
        let our_x3dh = X3DHKeyPair::generate();
        ExchangeSession {
            state: ExchangeState::Idle,
            mode: ExchangeMode::default(),
            transport: ExchangeTransport::Qr,
            identity,
            our_card,
            our_x3dh,
            proximity,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
        }
    }

    /// Creates a new NFC active exchange session.
    ///
    /// A single NFC tap replaces both QR scan and proximity verification.
    /// Both sides use fresh ephemeral X25519 keys for full forward secrecy.
    /// The session starts in `AwaitingNfcTap` — ready to receive a tap event.
    pub fn new_nfc(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        let our_x3dh = X3DHKeyPair::generate();
        ExchangeSession {
            state: ExchangeState::AwaitingNfcTap,
            mode: ExchangeMode::default(),
            transport: ExchangeTransport::Nfc,
            identity,
            our_card,
            our_x3dh,
            proximity,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
        }
    }

    /// Creates a new BLE exchange session.
    ///
    /// Uses GATT-based payload exchange with proximity verification.
    /// Both sides use fresh ephemeral X25519 keys for full forward secrecy.
    /// The session starts in `AwaitingBleConnection` — ready to receive a BLE event.
    pub fn new_ble(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        let our_x3dh = X3DHKeyPair::generate();
        ExchangeSession {
            state: ExchangeState::AwaitingBleConnection,
            mode: ExchangeMode::default(),
            transport: ExchangeTransport::Ble,
            identity,
            our_card,
            our_x3dh,
            proximity,
            started_at: Instant::now(),
            interrupted: false,
            used_qrs: HashSet::new(),
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

    /// Returns the QR code if in DisplayingQr or PeerScanned state.
    pub fn qr(&self) -> Option<&ExchangeQR> {
        match &self.state {
            ExchangeState::DisplayingQr { our_qr } => Some(our_qr),
            ExchangeState::PeerScanned { our_qr, .. } => Some(our_qr),
            _ => None,
        }
    }

    /// Returns the exchange mode.
    pub fn mode(&self) -> ExchangeMode {
        self.mode
    }

    /// Sets the exchange mode.
    pub fn set_mode(&mut self, mode: ExchangeMode) {
        self.mode = mode;
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
    pub fn apply(&mut self, event: ExchangeEvent) -> Result<(), ExchangeError> {
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
        }
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
        // HKDF is applied for domain separation (different IKM structure than full X3DH).
        let shared_bytes = self.our_x3dh.diffie_hellman(&their_exchange_key);
        let derived = HKDF::derive_key(None, &shared_bytes, b"vauchi-x3dh-symmetric-v1");
        let shared_key = crate::crypto::SymmetricKey::from_bytes(derived);

        self.state = ExchangeState::AwaitingCardExchange {
            their_public_key,
            shared_key,
        };

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

        let contact = Contact::from_exchange(their_public_key, their_card, shared_key);

        self.state = ExchangeState::Complete {
            contact: contact.clone(),
        };

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

        let our_qr = ExchangeQR::generate(&self.identity, &self.our_x3dh);
        self.state = ExchangeState::DisplayingQr { our_qr };
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

        self.state = ExchangeState::PeerScanned {
            our_qr,
            their_public_key,
            their_exchange_key,
        };

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
