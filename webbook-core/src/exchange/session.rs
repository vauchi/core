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
        their_qr: ExchangeQR,
    },
    /// Proximity verified, performing key agreement
    AwaitingKeyAgreement { their_public_key: [u8; 32] },
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
}

impl<P: ProximityVerifier> ExchangeSession<P> {
    /// Creates a new session as the initiator (displaying QR).
    pub fn new_initiator(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        ExchangeSession {
            state: ExchangeState::Idle,
            role: ExchangeRole::Initiator,
            identity,
            our_card,
            our_x3dh: X3DHKeyPair::generate(),
            proximity,
            started_at: Instant::now(),
            interrupted: false,
        }
    }

    /// Creates a new session as the responder (scanning QR).
    pub fn new_responder(identity: Identity, our_card: ContactCard, proximity: P) -> Self {
        ExchangeSession {
            state: ExchangeState::Idle,
            role: ExchangeRole::Responder,
            identity,
            our_card,
            our_x3dh: X3DHKeyPair::generate(),
            proximity,
            started_at: Instant::now(),
            interrupted: false,
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &ExchangeState {
        &self.state
    }

    /// Returns our role in the exchange.
    pub fn role(&self) -> ExchangeRole {
        self.role
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

    /// Generates a QR code for the exchange (initiator only).
    pub fn generate_qr(&mut self) -> Result<&ExchangeQR, ExchangeError> {
        if self.role != ExchangeRole::Initiator {
            return Err(ExchangeError::InvalidState(
                "Only initiator can generate QR".into(),
            ));
        }

        let qr = ExchangeQR::generate(&self.identity);
        self.state = ExchangeState::AwaitingScan { qr };

        match &self.state {
            ExchangeState::AwaitingScan { qr } => Ok(qr),
            _ => unreachable!(),
        }
    }

    /// Processes a scanned QR code (responder only).
    pub fn process_scanned_qr(&mut self, qr: ExchangeQR) -> Result<(), ExchangeError> {
        if self.role != ExchangeRole::Responder {
            return Err(ExchangeError::InvalidState(
                "Only responder can process QR".into(),
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

        self.state = ExchangeState::AwaitingProximity {
            their_public_key,
            their_qr: qr,
        };

        Ok(())
    }

    /// Performs proximity verification.
    pub fn verify_proximity(&mut self) -> Result<(), ExchangeError> {
        let (their_public_key, challenge) = match &self.state {
            ExchangeState::AwaitingProximity {
                their_public_key,
                their_qr,
            } => (*their_public_key, *their_qr.audio_challenge()),
            ExchangeState::AwaitingScan { qr } => {
                // Initiator waits for proximity challenge from responder
                (*qr.public_key(), *qr.audio_challenge())
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

        self.state = ExchangeState::AwaitingKeyAgreement { their_public_key };
        Ok(())
    }

    /// Performs X3DH key agreement.
    pub fn perform_key_agreement(&mut self) -> Result<(), ExchangeError> {
        let their_public_key = match &self.state {
            ExchangeState::AwaitingKeyAgreement { their_public_key } => *their_public_key,
            _ => {
                return Err(ExchangeError::InvalidState(
                    "Not in key agreement state".into(),
                ))
            }
        };

        let shared_key = match self.role {
            ExchangeRole::Initiator => {
                // Initiator generates ephemeral and sends to responder
                let (shared, _ephemeral) = X3DH::initiate(&self.our_x3dh, &their_public_key)?;
                shared
            }
            ExchangeRole::Responder => {
                // Responder uses initiator's ephemeral to derive same key
                // In real implementation, would receive ephemeral from initiator
                X3DH::respond(&self.our_x3dh, &[0u8; 32], &their_public_key)?
            }
        };

        self.state = ExchangeState::AwaitingCardExchange {
            their_public_key,
            shared_key,
        };

        Ok(())
    }

    /// Completes the exchange by exchanging cards.
    pub fn complete_exchange(&mut self, their_card: ContactCard) -> Result<Contact, ExchangeError> {
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
            | ExchangeState::AwaitingKeyAgreement { their_public_key }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::MockProximityVerifier;

    #[test]
    fn test_initiator_generates_qr() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let proximity = MockProximityVerifier::success();

        let mut session = ExchangeSession::new_initiator(identity, card, proximity);

        assert!(matches!(session.state(), ExchangeState::Idle));

        let qr = session.generate_qr().unwrap();
        assert!(!qr.is_expired());

        assert!(matches!(
            session.state(),
            ExchangeState::AwaitingScan { .. }
        ));
    }

    #[test]
    fn test_responder_processes_qr() {
        let alice_identity = Identity::create("Alice");
        let bob_identity = Identity::create("Bob");

        // Alice generates QR
        let alice_qr = ExchangeQR::generate(&alice_identity);

        // Bob processes it
        let bob_card = ContactCard::new("Bob");
        let proximity = MockProximityVerifier::success();
        let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

        bob_session.process_scanned_qr(alice_qr).unwrap();

        assert!(matches!(
            bob_session.state(),
            ExchangeState::AwaitingProximity { .. }
        ));
    }

    #[test]
    fn test_expired_qr_rejected() {
        let identity = Identity::create("Alice");
        let old_qr = ExchangeQR::generate_with_timestamp(
            &identity,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                - 360, // 6 minutes ago
        );

        let bob_identity = Identity::create("Bob");
        let bob_card = ContactCard::new("Bob");
        let proximity = MockProximityVerifier::success();
        let mut session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

        let result = session.process_scanned_qr(old_qr);
        assert!(matches!(result, Err(ExchangeError::QRExpired)));
    }

    #[test]
    fn test_session_timeout() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let proximity = MockProximityVerifier::success();

        let session = ExchangeSession::new_initiator(identity, card, proximity);

        // Fresh session should not be timed out
        assert!(!session.is_timed_out());
    }

    #[test]
    fn test_session_resume() {
        let identity = Identity::create("Alice");
        let card = ContactCard::new("Alice");
        let proximity = MockProximityVerifier::success();

        let mut session = ExchangeSession::new_initiator(identity, card, proximity);

        // Not interrupted yet
        assert!(!session.can_resume());

        // Mark as interrupted
        session.mark_interrupted();
        assert!(session.can_resume());
    }

    #[test]
    fn test_detect_duplicate_contact() {
        use crate::crypto::SymmetricKey;

        let alice_identity = Identity::create("Alice");
        let bob_identity = Identity::create("Bob");

        // Create an existing contact with Alice's public key
        let alice_card = ContactCard::new("Alice");
        let existing_alice = Contact::from_exchange(
            *alice_identity.signing_public_key(),
            alice_card.clone(),
            SymmetricKey::generate(),
        );

        let contacts = vec![existing_alice];

        // Bob scans Alice's QR
        let alice_qr = ExchangeQR::generate(&alice_identity);
        let bob_card = ContactCard::new("Bob");
        let proximity = MockProximityVerifier::success();
        let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

        bob_session.process_scanned_qr(alice_qr).unwrap();

        // Should detect Alice as duplicate
        let duplicate = bob_session.check_duplicate(&contacts);
        assert!(duplicate.is_some());
        assert_eq!(duplicate.unwrap().display_name(), "Alice");
    }

    #[test]
    fn test_no_duplicate_for_new_contact() {
        use crate::crypto::SymmetricKey;

        let alice_identity = Identity::create("Alice");
        let bob_identity = Identity::create("Bob");
        let charlie_identity = Identity::create("Charlie");

        // Create an existing contact with Charlie's public key
        let charlie_card = ContactCard::new("Charlie");
        let existing_charlie = Contact::from_exchange(
            *charlie_identity.signing_public_key(),
            charlie_card,
            SymmetricKey::generate(),
        );

        let contacts = vec![existing_charlie];

        // Bob scans Alice's QR (Alice is not in contacts)
        let alice_qr = ExchangeQR::generate(&alice_identity);
        let bob_card = ContactCard::new("Bob");
        let proximity = MockProximityVerifier::success();
        let mut bob_session = ExchangeSession::new_responder(bob_identity, bob_card, proximity);

        bob_session.process_scanned_qr(alice_qr).unwrap();

        // Should NOT detect a duplicate
        let duplicate = bob_session.check_duplicate(&contacts);
        assert!(duplicate.is_none());
    }

    #[test]
    fn test_duplicate_action_variants() {
        // Just verify the enum variants exist and can be compared
        assert_eq!(DuplicateAction::Update, DuplicateAction::Update);
        assert_ne!(DuplicateAction::Update, DuplicateAction::Keep);
        assert_ne!(DuplicateAction::Keep, DuplicateAction::Cancel);
    }
}
