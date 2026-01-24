//! Exchange Session State Machine
//!
//! Manages the state of a contact exchange from QR generation through
//! key agreement and card exchange.

use std::time::{Duration, Instant};

use super::{ExchangeError, ExchangeQR, ProximityVerifier, X3DHKeyPair, X3DH};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::identity::Identity;

/// Session timeout duration (60 seconds for resumption).
const SESSION_TIMEOUT: Duration = Duration::from_secs(60);

/// Default proximity verification timeout.
const PROXIMITY_TIMEOUT: Duration = Duration::from_secs(30);

/// State of an exchange session.
#[derive(Debug)]
pub enum ExchangeState {
    /// Initial state
    Idle,
    /// Displaying QR code, waiting for scan
    AwaitingScan { qr: ExchangeQR },
    /// QR scanned by other party, waiting for proximity verification
    AwaitingProximity {
        their_public_key: [u8; 32],
        their_exchange_key: [u8; 32],
        their_qr: ExchangeQR,
    },
    /// Proximity verified, performing key agreement
    AwaitingKeyAgreement {
        their_public_key: [u8; 32],
        their_exchange_key: [u8; 32],
    },
    /// Key agreement complete, exchanging cards
    AwaitingCardExchange {
        their_public_key: [u8; 32],
        shared_key: crate::crypto::SymmetricKey,
    },
    /// Exchange completed successfully
    Complete { contact: Contact },
    /// Exchange failed
    Failed { error: ExchangeError },
}

/// Events that drive the exchange state machine.
#[derive(Debug)]
pub enum ExchangeEvent {
    /// Initiator generates a QR code to be scanned.
    GenerateQR,
    /// Responder processes a scanned QR code.
    ProcessQR(ExchangeQR),
    /// Start or confirm proximity verification.
    VerifyProximity,
    /// Perform cryptographic key agreement.
    PerformKeyAgreement,
    /// Exchange contact cards and complete the session.
    CompleteExchange(ContactCard),
    /// Explicitly fail the session.
    Fail(ExchangeError),
}

/// Role in the exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangeRole {
    /// Initiated exchange by displaying QR
    Initiator,
    /// Responded by scanning QR
    Responder,
}

/// An exchange session managing the state of a contact exchange.
pub struct ExchangeSession<P: ProximityVerifier> {
    /// Current state
    state: ExchangeState,
    /// Our role in the exchange
    role: ExchangeRole,
    /// Our identity
    identity: Identity,
    /// Our contact card to share
    our_card: ContactCard,
    /// Our X3DH keypair for this session
    our_x3dh: X3DHKeyPair,
    /// Proximity verifier
    proximity: P,
    /// When the session started
    started_at: Instant,
    /// Whether the session was interrupted
    interrupted: bool,
    /// Our ephemeral public key (set when we're the scanner/X3DH initiator)
    our_ephemeral: Option<[u8; 32]>,
    /// Their ephemeral public key (received when we're the displayer/X3DH responder)
    their_ephemeral: Option<[u8; 32]>,
}

impl<P: ProximityVerifier> ExchangeSession<P> {
    /// Creates a new session as the initiator (displaying QR).
    pub fn new_initiator(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        // Use identity's X3DH keypair so our exchange key matches QR
        let our_x3dh = identity.x3dh_keypair();
        ExchangeSession {
            state: ExchangeState::Idle,
            role: ExchangeRole::Initiator,
            identity,
            our_card,
            our_x3dh,
            proximity,
            started_at: Instant::now(),
            interrupted: false,
            our_ephemeral: None,
            their_ephemeral: None,
        }
    }

    /// Creates a new session as the responder (scanning QR).
    pub fn new_responder(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        // Responder (scanner) generates ephemeral, doesn't need identity's X3DH
        // but we keep it for consistency
        let our_x3dh = identity.x3dh_keypair();
        ExchangeSession {
            state: ExchangeState::Idle,
            role: ExchangeRole::Responder,
            identity,
            our_card,
            our_x3dh,
            proximity,
            started_at: Instant::now(),
            interrupted: false,
            our_ephemeral: None,
            their_ephemeral: None,
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &ExchangeState {
        &self.state
    }

    /// Returns the QR code if in AwaitingScan state.
    pub fn qr(&self) -> Option<&ExchangeQR> {
        match &self.state {
            ExchangeState::AwaitingScan { qr } => Some(qr),
            _ => None,
        }
    }

    /// Returns our role in the exchange.
    pub fn role(&self) -> ExchangeRole {
        self.role
    }

    /// Returns our ephemeral public key (if we're the scanner/X3DH initiator).
    ///
    /// This should be sent to the QR displayer after key agreement.
    pub fn ephemeral_public(&self) -> Option<[u8; 32]> {
        self.our_ephemeral
    }

    /// Sets their ephemeral public key (if we're the displayer/X3DH responder).
    ///
    /// This must be called before key agreement when we're the QR displayer.
    pub fn set_their_ephemeral(&mut self, ephemeral: [u8; 32]) {
        self.their_ephemeral = Some(ephemeral);
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
            ExchangeEvent::GenerateQR => self.handle_generate_qr(),
            ExchangeEvent::ProcessQR(qr) => self.handle_process_qr(qr),
            ExchangeEvent::VerifyProximity => self.handle_verify_proximity(),
            ExchangeEvent::PerformKeyAgreement => self.handle_perform_key_agreement(),
            ExchangeEvent::CompleteExchange(card) => {
                self.handle_complete_exchange(card).map(|_| ())
            }
            ExchangeEvent::Fail(err) => {
                self.fail(err);
                Ok(())
            }
        }
    }

    fn handle_generate_qr(&mut self) -> Result<(), ExchangeError> {
        if self.role != ExchangeRole::Initiator {
            return Err(ExchangeError::InvalidState(
                "Only initiator can generate QR".into(),
            ));
        }

        if !matches!(self.state, ExchangeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Can only generate QR from Idle state".into(),
            ));
        }

        let qr = ExchangeQR::generate(&self.identity);
        self.state = ExchangeState::AwaitingScan { qr };
        Ok(())
    }

    fn handle_process_qr(&mut self, qr: ExchangeQR) -> Result<(), ExchangeError> {
        if self.role != ExchangeRole::Responder {
            return Err(ExchangeError::InvalidState(
                "Only responder can process QR".into(),
            ));
        }

        if !matches!(self.state, ExchangeState::Idle) {
            return Err(ExchangeError::InvalidState(
                "Can only process QR from Idle state".into(),
            ));
        }

        // Verify QR code
        if qr.is_expired() {
            return Err(ExchangeError::QRExpired);
        }

        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *qr.public_key();
        let their_exchange_key = *qr.exchange_key();

        // Check for self-exchange (scanning own QR code)
        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        self.state = ExchangeState::AwaitingProximity {
            their_public_key,
            their_exchange_key,
            their_qr: qr,
        };

        Ok(())
    }

    fn handle_verify_proximity(&mut self) -> Result<(), ExchangeError> {
        let (their_public_key, their_exchange_key, challenge) = match &self.state {
            ExchangeState::AwaitingProximity {
                their_public_key,
                their_exchange_key,
                their_qr,
            } => (
                *their_public_key,
                *their_exchange_key,
                *their_qr.audio_challenge(),
            ),
            ExchangeState::AwaitingScan { qr } => {
                // Initiator waits for proximity challenge from responder
                // Initiator doesn't have their exchange key yet - will receive ephemeral
                (*qr.public_key(), [0u8; 32], *qr.audio_challenge())
            }
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Not in proximity verification state".into(),
                ))
            }
        };

        self.proximity
            .verify_proximity(&challenge, PROXIMITY_TIMEOUT)
            .map_err(|_| ExchangeError::ProximityFailed)?;

        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
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

        let shared_key = match self.role {
            ExchangeRole::Responder => {
                // Responder (QR scanner) is the X3DH INITIATOR:
                // - Has their exchange key from the QR
                // - Generates ephemeral, stores it for transfer to displayer
                let (shared, ephemeral) = X3DH::initiate(&self.our_x3dh, &their_exchange_key)?;
                self.our_ephemeral = Some(ephemeral);
                shared
            }
            ExchangeRole::Initiator => {
                // Initiator (QR displayer) is the X3DH RESPONDER:
                // - Needs the scanner's ephemeral (received via their_ephemeral)
                // - Uses own X3DH keys to derive shared secret
                let their_ephemeral = self.their_ephemeral.ok_or_else(|| {
                    ExchangeError::InvalidState(
                        "Missing ephemeral from scanner - call set_their_ephemeral first".into(),
                    )
                })?;
                X3DH::respond(&self.our_x3dh, &[0u8; 32], &their_ephemeral)?
            }
        };

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

    /// Returns our card (for sending to the other party).
    pub fn our_card(&self) -> &ContactCard {
        &self.our_card
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
            ExchangeState::AwaitingProximity {
                their_public_key, ..
            }
            | ExchangeState::AwaitingKeyAgreement {
                their_public_key, ..
            }
            | ExchangeState::AwaitingCardExchange {
                their_public_key, ..
            } => Some(their_public_key),
            _ => None,
        };

        their_key.and_then(|key| contacts.iter().find(|c| c.public_key() == key))
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
