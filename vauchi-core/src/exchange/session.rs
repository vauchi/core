// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Session State Machine
//!
//! Manages the state of a contact exchange from QR generation through
//! key agreement and card exchange.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::monotonic::{MonotonicClock, SystemMonotonicClock};

use super::ble_handshake::BleHandshakeSession;
use super::ble_payload::BleCardPayload;
use super::direct_transport::UsbRole;
use super::nfc_handshake::NfcHandshakeSession;
use super::trust_metrics::TrustMetrics;
use super::{ExchangeError, ExchangeQR, ProximityConfidence, ProximityVerifier, X3DHKeyPair};
use crate::contact::Contact;
use crate::contact_card::ContactCard;
use crate::crypto::kdf::HKDF;

/// HKDF domain-separation label for the USB/direct-transport card-exchange key
/// (ADR-007). Bumping `v1` is a wire break.
const USB_CARD_EXCHANGE_INFO: &[u8] = b"vauchi/usb-card-exchange/v1";
use crate::diagnostic::exchange_debug::{ExchangeDebugEvent, ExchangeDebugLog};
use crate::identity::Identity;
use crate::{Command, Event};

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
    /// Direct transport (USB/TCP): ready for payload exchange.
    /// `emit_initial_commands()` emits `DirectSend` with our payload.
    /// Frontend executes TCP exchange and reports the peer's payload
    /// via `Event::DirectPayloadReceived`.
    AwaitingDirectPayload { our_qr: ExchangeQR },
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

    /// NFC tap completed; contains their payload bytes.
    NfcTapComplete { their_payload: Vec<u8> },

    /// Start a BLE exchange (begin advertising/scanning).
    StartBleExchange,
    /// BLE payloads exchanged; contains their payload bytes and device ID.
    BlePayloadExchanged {
        their_payload: Vec<u8>,
        device_id: String,
    },
    /// BLE proximity verified (challenge-response passed).
    BleProximityVerified,

    /// Proximity check completed with a confidence level.
    ProximityCheckCompleted { confidence: ProximityConfidence },
}

/// An exchange session managing the state of a contact exchange.
///
/// Stores the proximity verifier as `Box<dyn ProximityVerifier>`, eliminating
/// the need for generic type parameters and enabling any verifier backend to be
/// used directly without enum dispatch wrappers.
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
    /// The peer's X25519 exchange (DH) public key, captured at key agreement.
    /// Retained past `AwaitingCardExchange` so the post-`Complete` save sites
    /// can initialize the Double Ratchet against the *exchange* key the X3DH
    /// secret was derived from — not the Ed25519 identity key. `None` until key
    /// agreement runs. See `build_exchange_ratchet`.
    their_exchange_key: Option<[u8; 32]>,
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
    /// Role in a USB/TCP direct exchange (Initiator sends first, Responder receives first).
    /// `None` for non-USB transports.
    usb_role: Option<UsbRole>,
    /// Whether we initiated the BLE connection (scanner role).
    /// Set to `true` on `BleDeviceDiscovered`, determines who sends KeyOffer first.
    ble_is_initiator: bool,
    /// Buffered BLE handshake data (KeyAck or commitment) awaiting card data.
    ble_pending_handshake: Option<Vec<u8>>,
    /// Buffered BLE encrypted card data awaiting handshake data.
    ble_pending_card: Option<Vec<u8>>,
    /// Our relay URL to include in QR code (for per-contact routing).
    our_relay_url: Option<String>,
    /// Their relay URL extracted from their QR code.
    their_relay_url: Option<String>,
    /// Optional exchange debug log. When enabled, captures timestamped
    /// events at each state transition for diagnostic analysis.
    debug_log: Option<ExchangeDebugLog>,
    /// Pending commands to be sent to the frontend (ADR-031).
    /// Populated by `apply_hardware_event()` and drained by `drain_commands()`.
    pending_commands: Vec<Command>,
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
    /// Clock for stamping timestamps on peer-card field constructions
    /// during BLE phase 4 and on every payload / QR / NFC generation.
    /// Defaults to `crate::clock::SystemClock::shared()` in every
    /// `new_*` constructor; tests can override via `with_clock(...)`
    /// after construction. Phase 1 / Task 1.1 / Step 3b structural pass.
    clock: Arc<dyn crate::clock::Clock>,
    /// Explicit-monotonic-time seam (Phase 1 / Task 1.1b). Stamps
    /// `started_at` and backs the `is_timed_out` `SESSION_TIMEOUT`
    /// comparison. Defaults to `SystemMonotonicClock::shared()` in every
    /// `new_*` constructor; tests override via `with_monotonic(...)`,
    /// which re-stamps `started_at` so the start and the timeout check
    /// share one clock domain.
    monotonic: Arc<dyn MonotonicClock>,
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
        clock: Arc<dyn crate::clock::Clock>,
    ) -> Self {
        // Fresh ephemeral keypair — NOT derived from identity
        let our_x3dh = X3DHKeyPair::generate();
        ExchangeSession {
            state: ExchangeState::Idle,
            transport: ExchangeTransport::Qr,
            identity,
            our_card,
            our_x3dh,
            their_exchange_key: None,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: SystemMonotonicClock::new().now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: None,
            device_capabilities: None,
            usb_role: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            their_relay_url: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
            clock,
            monotonic: SystemMonotonicClock::shared(),
        }
    }

    /// Restores a QR exchange started by an earlier process invocation.
    ///
    /// The caller must persist the original QR and its ephemeral secret in
    /// restricted local storage, then delete both after completion or expiry.
    /// The pair is validated against the identity, signature, expiry, and
    /// X25519 public key before the session is restored to `DisplayingQr`.
    /// Reusing this exact ephemeral is required for both peers to derive the
    /// same forward-secret channel when `start` and `complete` are separate
    /// CLI commands.
    pub fn resume_qr(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
        ephemeral_secret: [u8; 32],
        our_qr: ExchangeQR,
        clock: Arc<dyn crate::clock::Clock>,
    ) -> Result<Self, ExchangeError> {
        if !our_qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }
        if our_qr.is_expired(clock.unix_seconds()) {
            return Err(ExchangeError::QRExpired);
        }
        if our_qr.public_key() != identity.signing_public_key() {
            return Err(ExchangeError::InvalidState(
                "pending QR belongs to a different identity".into(),
            ));
        }

        let our_x3dh = X3DHKeyPair::from_bytes(ephemeral_secret);
        if our_qr.exchange_key() != our_x3dh.public_key() {
            return Err(ExchangeError::InvalidState(
                "pending QR does not match its ephemeral secret".into(),
            ));
        }

        let our_audio_challenge = Some(*our_qr.audio_challenge());
        let our_relay_url = our_qr.relay_url().map(String::from);
        let mut session = Self::new_qr(identity, our_card, proximity, clock);
        session.our_x3dh = our_x3dh;
        session.our_audio_challenge = our_audio_challenge;
        session.our_relay_url = our_relay_url;
        session.state = ExchangeState::DisplayingQr { our_qr };
        Ok(session)
    }

    /// Test-only constructor that accepts a specific X3DH keypair for deterministic testing.
    #[cfg(any(test, feature = "testing"))]
    pub fn new_qr_with_x3dh(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
        our_x3dh: X3DHKeyPair,
        clock: Arc<dyn crate::clock::Clock>,
    ) -> Self {
        ExchangeSession {
            state: ExchangeState::Idle,
            transport: ExchangeTransport::Qr,
            identity,
            our_card,
            our_x3dh,
            their_exchange_key: None,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: SystemMonotonicClock::new().now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: None,
            device_capabilities: None,
            usb_role: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            their_relay_url: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
            clock,
            monotonic: SystemMonotonicClock::shared(),
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
        clock: Arc<dyn crate::clock::Clock>,
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
            their_exchange_key: None,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: SystemMonotonicClock::new().now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: Some(nfc_handshake),
            ble_handshake: None,
            device_capabilities: None,
            usb_role: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            their_relay_url: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
            clock,
            monotonic: SystemMonotonicClock::shared(),
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
        clock: Arc<dyn crate::clock::Clock>,
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
        let ble_handshake =
            BleHandshakeSession::new_initiator(&identity, ble_card, clock.unix_seconds());
        ExchangeSession {
            state: ExchangeState::AwaitingBleConnection,
            transport: ExchangeTransport::Ble,
            identity,
            our_card,
            our_x3dh,
            their_exchange_key: None,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: SystemMonotonicClock::new().now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: None,
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: Some(ble_handshake),
            device_capabilities: None,
            usb_role: None,
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            their_relay_url: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
            clock,
            monotonic: SystemMonotonicClock::shared(),
        }
    }

    /// Creates a new USB/TCP direct transport exchange session.
    ///
    /// Used for desktop-to-phone exchange over a physical USB cable or
    /// local network TCP connection. The session starts in
    /// `AwaitingDirectPayload`. The ADR-031 command/event flow is:
    /// 1. Call `emit_initial_commands()` → emits `DirectSend` with our payload
    /// 2. Frontend executes TCP exchange, receives peer's payload
    /// 3. Frontend reports `Event::DirectPayloadReceived`
    /// 4. Then `PerformKeyAgreement` → `CompleteExchange` as usual
    pub fn new_usb(
        identity: Identity,
        our_card: ContactCard,
        proximity: impl ProximityVerifier + 'static,
        role: UsbRole,
        clock: Arc<dyn crate::clock::Clock>,
    ) -> Self {
        let our_x3dh = X3DHKeyPair::generate();
        let our_qr = ExchangeQR::generate(&identity, &our_x3dh, clock.unix_seconds());
        ExchangeSession {
            state: ExchangeState::AwaitingDirectPayload {
                our_qr: our_qr.clone(),
            },
            transport: ExchangeTransport::Usb,
            identity,
            our_card,
            our_x3dh,
            their_exchange_key: None,
            proximity: Box::new(proximity),
            proximity_confidence: ProximityConfidence::Unknown,
            started_at: SystemMonotonicClock::new().now(),
            interrupted: false,
            used_qrs: HashSet::new(),
            their_audio_challenge: None,
            our_audio_challenge: Some(*our_qr.audio_challenge()),
            their_display_name: None,
            nfc_handshake: None,
            ble_handshake: None,
            device_capabilities: None,
            usb_role: Some(role),
            ble_is_initiator: false,
            ble_pending_handshake: None,
            ble_pending_card: None,
            our_relay_url: None,
            their_relay_url: None,
            debug_log: None,
            pending_commands: Vec::new(),
            our_confirmation_token: None,
            expected_their_token: None,
            confirmation_gate_hash: None,
            confirmation_our_slot: None,
            confirmation_their_slot: None,
            clock,
            monotonic: SystemMonotonicClock::shared(),
        }
    }

    /// Returns the exchange payload data string for sending over a direct transport.
    ///
    /// Only available in `AwaitingDirectPayload` state. The returned string
    /// is the same format as QR data (base64-encoded, signed).
    pub fn our_exchange_payload(&self) -> Option<String> {
        match &self.state {
            ExchangeState::AwaitingDirectPayload { our_qr } => Some(our_qr.to_data_string()),
            _ => None,
        }
    }

    /// Returns the current state.
    pub fn state(&self) -> &ExchangeState {
        &self.state
    }

    /// Returns `true` if the exchange completed successfully.
    pub fn is_complete(&self) -> bool {
        matches!(self.state, ExchangeState::Complete { .. })
    }

    /// Returns `true` if the exchange failed.
    pub fn is_failed(&self) -> bool {
        matches!(self.state, ExchangeState::Failed { .. })
    }

    /// Returns the failure reason, if the session is in `Failed` state.
    pub fn failure_reason(&self) -> Option<&ExchangeError> {
        match &self.state {
            ExchangeState::Failed { error } => Some(error),
            _ => None,
        }
    }

    /// Extracts the completed contact, consuming the `Complete` state.
    ///
    /// Returns `Some(contact)` if the session is in `Complete` state,
    /// resetting the state to `Idle`. Returns `None` otherwise.
    pub fn extract_contact(&mut self) -> Option<Contact> {
        match std::mem::replace(&mut self.state, ExchangeState::Idle) {
            ExchangeState::Complete { contact } => Some(*contact),
            other => {
                self.state = other;
                None
            }
        }
    }

    /// Builds the role-correct Double Ratchet for a completed exchange.
    ///
    /// This is the single seam every in-person (non-relay) save site must use
    /// to initialize the ratchet. The relay flow assigns the X3DH role by which
    /// method runs (claim = initiator, complete = responder); symmetric
    /// in-person exchange (QR/BLE/NFC) has no such split, so the role is derived
    /// deterministically from the two identity keys -- smaller = initiator --
    /// the same rule used for `EscrowRole` in `handle_perform_key_agreement`.
    /// The initiator keys the ratchet off the peer's X25519 *exchange* key
    /// (retained in `their_exchange_key` at key agreement); the responder keys
    /// it off our own exchange keypair (`our_x3dh`) -- the keypair whose public
    /// key the initiator received. Both sides reconcile on the first message
    /// (see `DoubleRatchetState::dh_ratchet`).
    ///
    /// Returns the initialized ratchet and the `is_initiator` flag for
    /// `Storage::save_ratchet_state`. Pure crypto -- persistence stays with the
    /// caller (ADR-031).
    ///
    /// Why this exists: feeding `contact.public_key()` (the Ed25519 identity
    /// key) as the DH key, or initializing both peers as initiator, silently
    /// produces an undecryptable channel. Routing every save site through this
    /// method makes both mistakes unrepresentable.
    ///
    /// `contact` is the completed exchange contact; it supplies the shared
    /// secret and the peer's identity key (for the role decision).
    pub fn build_exchange_ratchet(
        &self,
        contact: &Contact,
    ) -> Result<(crate::crypto::DoubleRatchetState, bool), ExchangeError> {
        let shared_key = contact.shared_key().ok_or_else(|| {
            ExchangeError::InvalidState("exchange contact has no shared key".into())
        })?;
        let their_identity = contact.public_key().ok_or_else(|| {
            ExchangeError::InvalidState("exchange contact has no identity key".into())
        })?;
        let their_exchange_key = self.their_exchange_key.ok_or_else(|| {
            ExchangeError::InvalidState(
                "no peer exchange key retained (key agreement not performed)".into(),
            )
        })?;

        let our_dh = X3DHKeyPair::from_bytes(*self.our_x3dh.secret_bytes());
        super::ratchet_bootstrap::bootstrap_exchange_ratchet(
            shared_key,
            self.identity.signing_public_key(),
            their_identity,
            Some(their_exchange_key),
            Some(our_dh),
        )
        .map_err(|e| match e {
            super::ratchet_bootstrap::RatchetBootstrapError::Init(msg) => {
                ExchangeError::KeyAgreementFailed(msg)
            }
            // Both ephemerals are resolved above, so Missing* cannot occur.
            other => ExchangeError::InvalidState(format!("ratchet bootstrap: {other:?}")),
        })
    }

    /// Returns the shared key from the `AwaitingCardExchange` state.
    ///
    /// Useful for card exchange setup before calling `CompleteExchange`.
    pub fn shared_key(&self) -> Option<&crate::crypto::SymmetricKey> {
        match &self.state {
            ExchangeState::AwaitingCardExchange { shared_key, .. } => Some(shared_key),
            _ => None,
        }
    }

    /// Returns the transport mechanism.
    /// Replaces the session's clock — for deterministic tests.
    ///
    /// Defaults to `crate::clock::SystemClock::shared()` from every
    /// `new_*` constructor; tests can override post-construction.
    /// Phase 1 / Task 1.1 / Step 3b structural pass (initial seam).
    pub fn with_clock(mut self, clock: Arc<dyn crate::clock::Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// Replaces the session's monotonic clock — for deterministic
    /// timeout tests (Phase 1 / Task 1.1b). Re-stamps `started_at` from
    /// the injected clock so the recorded start and the `is_timed_out`
    /// comparison share one clock domain; a `FakeMonotonicClock` can
    /// then drive `SESSION_TIMEOUT` purely by `advance`.
    #[must_use]
    pub fn with_monotonic(mut self, monotonic: Arc<dyn MonotonicClock>) -> Self {
        self.started_at = monotonic.now();
        self.monotonic = monotonic;
        self
    }

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

    /// Returns the QR code if in DisplayingQr or PeerScanned state.
    pub fn qr(&self) -> Option<&ExchangeQR> {
        match &self.state {
            ExchangeState::DisplayingQr { our_qr } => Some(our_qr),
            ExchangeState::PeerScanned { our_qr, .. } => Some(our_qr),
            _ => None,
        }
    }

    /// Returns the ephemeral QR secret needed to resume this pending session.
    ///
    /// The secret is wrapped in [`zeroize::Zeroizing`] and is available only
    /// while the QR session still retains its displayed QR. Callers persisting
    /// it must use restricted local storage and remove it after completion or
    /// expiry.
    pub fn qr_resume_secret(&self) -> Option<zeroize::Zeroizing<[u8; 32]>> {
        self.qr().map(|_| self.our_x3dh.secret_bytes())
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

    /// Builds TrustMetrics from the current session state.
    ///
    /// Called internally when creating a contact after successful exchange.
    fn build_trust_metrics(&self) -> TrustMetrics {
        let timestamp = self.clock.unix_seconds();
        TrustMetrics::new(self.transport, self.proximity_confidence, timestamp)
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
            ExchangeTransport::MultiStage => "multi_stage",
            ExchangeTransport::Link => "link",
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
        self.monotonic.now().duration_since(self.started_at) > SESSION_TIMEOUT
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
            ExchangeEvent::StartQR => self.handle_start_qr(),
            ExchangeEvent::ProcessQR(qr) => self.handle_process_qr(qr),
            ExchangeEvent::TheyScannedOurQR => self.handle_they_scanned_our_qr(),
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
            ExchangeEvent::NfcTapComplete { their_payload } => {
                self.handle_nfc_tap_complete(their_payload)
            }
            ExchangeEvent::StartBleExchange => self.handle_start_ble_exchange(),
            ExchangeEvent::BlePayloadExchanged {
                their_payload,
                device_id,
            } => self.handle_ble_payload_exchanged(their_payload, device_id),
            ExchangeEvent::BleProximityVerified => self.handle_ble_proximity_verified(),
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
    pub fn drain_commands(&mut self) -> Vec<Command> {
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
    fn emit_command(&mut self, cmd: Command) {
        self.debug_event(ExchangeDebugEvent::CommandDispatched {
            command_name: cmd.variant_name().to_string(),
        });
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
    pub fn apply_hardware_event(&mut self, event: Event) -> Result<(), ExchangeError> {
        match event {
            Event::QrScanned { data } => self.handle_qr_scanned(data),
            Event::NfcDataReceived { data } => self.handle_nfc_data_received(data),
            Event::BleConnected { device_id } => self.handle_ble_connected(device_id),
            Event::BleCharacteristicRead { uuid, data }
            | Event::BleCharacteristicNotified { uuid, data } => {
                self.handle_ble_characteristic_data(uuid, data)
            }
            Event::BleDeviceDiscovered { id, .. } => self.handle_ble_device_discovered(id),
            Event::BleDisconnected { reason } => self.handle_ble_disconnected(reason),
            Event::HardwareError { transport, error } => {
                self.handle_hardware_error(transport, error)
            }
            Event::HardwareUnavailable { transport } => self.handle_hardware_unavailable(transport),
            Event::PermissionDenied { transport } => self.handle_permission_denied(transport),
            Event::DirectPayloadReceived { data } => {
                let payload_str =
                    String::from_utf8(data).map_err(|_| ExchangeError::InvalidQRFormat)?;
                self.handle_direct_payload_received(payload_str)
            }
            Event::DirectCardReceived { ciphertext } => {
                self.handle_direct_card_received(ciphertext)
            }
            Event::AudioSamplesRecorded { .. } => {
                // Audio proximity response — trigger proximity check.
                // Decoding lives in ProximityRunner; the session just
                // ack-tracks that something arrived.
                Ok(())
            }
            ignored => self.handle_ignored_event(&ignored),
        }
    }

    /// Handles a QR scan event by parsing the data and initiating the exchange.
    fn handle_qr_scanned(&mut self, data: String) -> Result<(), ExchangeError> {
        let qr = ExchangeQR::from_data_string(&data)?;
        self.apply(ExchangeEvent::ProcessQR(qr))
    }

    /// Handles NFC data received from a tap exchange.
    fn handle_nfc_data_received(&mut self, data: Vec<u8>) -> Result<(), ExchangeError> {
        let result = self.apply(ExchangeEvent::NfcTapComplete {
            their_payload: data,
        });
        // Deactivate NFC interface after tap is processed
        if result.is_ok() {
            self.emit_command(Command::NfcDeactivate);
        }
        result
    }

    /// Handles BLE peer discovery — stop scanning and initiate connection.
    fn handle_ble_device_discovered(&mut self, id: String) -> Result<(), ExchangeError> {
        self.ble_is_initiator = true;
        self.emit_command(Command::BleStopScanning);
        self.emit_command(Command::BleConnect { device_id: id });
        Ok(())
    }

    /// Handles BLE disconnection. If we were awaiting connection, fail the exchange.
    fn handle_ble_disconnected(&mut self, reason: String) -> Result<(), ExchangeError> {
        if matches!(self.state, ExchangeState::AwaitingBleConnection) {
            self.apply(ExchangeEvent::Fail(ExchangeError::BleConnectionLost))?;
        }
        self.debug_event(ExchangeDebugEvent::ExchangeFailed {
            error: format!("BLE disconnected: {}", reason),
        });
        Ok(())
    }

    /// Handles a hardware error by failing the exchange.
    fn handle_hardware_error(
        &mut self,
        transport: String,
        error: String,
    ) -> Result<(), ExchangeError> {
        self.apply(ExchangeEvent::Fail(ExchangeError::HardwareFailure {
            transport,
            error,
        }))
    }

    /// Handles hardware unavailable by logging and attempting transport fallback.
    fn handle_hardware_unavailable(&mut self, transport: String) -> Result<(), ExchangeError> {
        self.debug_event(ExchangeDebugEvent::ExchangeFailed {
            error: format!("{} hardware unavailable", transport),
        });
        self.attempt_transport_fallback(&transport);
        Ok(())
    }

    /// Handles permission denied by logging and attempting transport fallback.
    fn handle_permission_denied(&mut self, transport: String) -> Result<(), ExchangeError> {
        self.debug_event(ExchangeDebugEvent::ExchangeFailed {
            error: format!("{} permission denied", transport),
        });
        // Same fallback logic — the transport can't be used regardless
        // of whether hardware is absent or permission was denied.
        self.attempt_transport_fallback(&transport);
        Ok(())
    }

    /// Handles events that the exchange session state machine intentionally ignores.
    ///
    /// These are platform events that are valid but not relevant to the exchange
    /// protocol. Each variant has a documented reason for being ignored.
    fn handle_ignored_event(&mut self, event: &Event) -> Result<(), ExchangeError> {
        match event {
            // TapHoverShake accelerometer events — proximity verification is handled
            // by the BleExchangeFlow / MultiStageExchangeEngine, not the legacy session.
            Event::AccelerometerData { .. } | Event::ImpactDetected { .. } => {}
            // Relay escrow events are handled by the link-exchange engine (ADR-049).
            Event::RelayEscrowReady { .. }
            | Event::RelayEscrowBlobReceived { .. }
            | Event::RelayEscrowFailed { .. }
            | Event::LinkShared
            | Event::LinkOpened { .. } => {}
            // Image picking events are for the avatar editor, not exchanges.
            Event::ImageReceived { .. } | Event::ImagePickCancelled => {}
            // QR scan progress is a UI-only signal handled by ExchangeEngine's
            // ScanQualityTracker — the session state machine ignores it.
            Event::QrScanProgress { .. } => {}
            // File picking events drive vCard / backup import in vauchi-app.
            Event::FilePickedFromUser { .. } | Event::FilePickCancelledByUser => {}
            // Biometric unlock event is auth-layer; the exchange state machine ignores it.
            Event::BiometricUnlockSucceeded => {}
            // BLE MTU negotiation is a transport-layer signal consumed by the binding's
            // GATT chunker. The core session has no opinion on it.
            Event::BleMtuNegotiated { .. } => {}
            // LocationResult is a contact annotation (ADR-051 "where we met"), not a
            // handshake event. The annotation layer captures it at exchange completion.
            Event::LocationResult { .. } => {}
            // All other events are not expected by the exchange session.
            _ => {}
        }
        Ok(())
    }

    /// Emits initial commands for the current transport type.
    ///
    /// Call this after creating a new session to get the first set of
    /// hardware commands (e.g., `QrDisplay` for QR sessions, `BleStartScanning`
    /// for BLE sessions). Use `drain_commands()` to retrieve them.
    pub fn emit_initial_commands(&mut self) {
        // Build commands first to avoid borrowing conflicts with emit_command().
        let cmds: Vec<Command> = match (&self.state, self.transport) {
            (ExchangeState::DisplayingQr { our_qr }, ExchangeTransport::Qr) => {
                vec![Command::QrDisplay {
                    data: our_qr.to_data_string(),
                }]
            }
            (ExchangeState::AwaitingNfcTap, ExchangeTransport::Nfc) => {
                // Generate our NFC key offer payload for the frontend to present.
                // The frontend activates the NFC interface with this data, and when
                // the peer taps, sends their data back as NfcDataReceived.
                let now = self.clock.unix_seconds();
                let payload = if let Some(ref mut hs) = self.nfc_handshake {
                    hs.create_key_offer(&self.identity, now).unwrap_or_default()
                } else {
                    // Fallback: generate ExchangeNfc payload directly
                    let nfc = super::nfc_active::ExchangeNfc::generate(
                        &self.identity,
                        &self.our_x3dh,
                        now,
                    );
                    nfc.to_bytes().to_vec()
                };
                vec![Command::NfcActivate { payload }]
            }
            (ExchangeState::AwaitingBleConnection, ExchangeTransport::Ble) => {
                vec![
                    Command::BleStartScanning {
                        service_uuid: super::VAUCHI_BLE_SERVICE_UUID.to_string(),
                    },
                    Command::BleStartAdvertising {
                        service_uuid: super::VAUCHI_BLE_SERVICE_UUID.to_string(),
                        payload: Vec::new(),
                    },
                ]
            }
            (ExchangeState::AwaitingDirectPayload { our_qr }, ExchangeTransport::Usb) => {
                vec![Command::DirectSend {
                    payload: our_qr.to_data_string().into_bytes(),
                    is_initiator: self.usb_role == Some(UsbRole::Initiator),
                }]
            }
            _ => vec![],
        };
        for cmd in cmds {
            self.emit_command(cmd);
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
        // report AudioSamplesRecorded to upgrade confidence. Frontends that
        // don't support audio simply ignore them (confidence stays at baseline).
        if let Some(their) = self.their_audio_challenge {
            let is_initiator = self.their_audio_challenge.is_some();
            let modem_config = crate::exchange::audio_modem::AudioConfig::default();
            let modem_rate = modem_config.sample_rate;
            let samples = crate::exchange::audio_modem::generate_fsk_samples(&their, &modem_config);
            let emit = Command::AudioEmitChallenge {
                samples,
                sample_rate: modem_rate,
            };
            let listen = Command::AudioListenForResponse {
                timeout_ms: 5000,
                sample_rate: modem_rate,
            };
            if is_initiator {
                self.emit_command(emit);
                self.emit_command(listen);
            } else {
                self.emit_command(listen);
                self.emit_command(emit);
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
        self.emit_command(Command::BleWriteCharacteristic {
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
            let (our_commitment, our_encrypted_card) =
                hs.process_key_ack(&key_ack, &their_card, self.clock.unix_seconds())?;

            self.emit_command(Command::BleWriteCharacteristic {
                uuid: super::CHAR_HANDSHAKE_WRITE.to_string(),
                data: our_commitment,
            });
            self.emit_command(Command::BleWriteCharacteristic {
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
            self.emit_command(Command::BleWriteCharacteristic {
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
            // BLE payload is parser-validated for length; add_field can
            // still fail on MAX_FIELDS / Validation. ADR-042-shape lenient:
            // keep the peer card, drop the failing field — but surface
            // for operators (corrupt-payload rates from peers).
            // PII-safe: ContactCardError variants carry no field value
            // or label.
            if let Err(e) = their_card.add_field(crate::contact_card::ContactField::new(
                crate::contact_card::FieldType::Custom,
                label,
                value,
                self.clock.unix_seconds(),
            )) {
                // ADR-042-shape lenient: keep the peer card, drop only the
                // failing field. Log at debug level — the caller decides
                // whether to surface warnings to the user.
                tracing::debug!(
                    error = %e,
                    "ble exchange: dropping peer field that failed validation"
                );
            }
        }
        if let Some(ref avatar) = remote.avatar
            && let Err(e) = their_card.set_avatar(avatar.clone())
        {
            // ADR-042: peer-supplied avatar bytes go through the same
            // WebP normalization as local avatars. A failure here means
            // the peer's payload didn't decode or didn't fit within the
            // size cap. Lenient: keep the contact, drop only the avatar
            // — but log so we see corrupt-payload rates from peers.
            // PII-safe: ContactCardError variants do not include the
            // avatar bytes or any peer identifier.
            // Log at debug level — the caller decides whether to surface
            // warnings to the user.
            tracing::debug!(
                error = %e,
                "ble exchange: dropping peer avatar that failed normalization (ADR-042)"
            );
        }

        // Derive a relay-use shared key from both parties' identity keys.
        // This is deterministic — both sides compute the same key. Used for
        // relay message encryption, NOT for BLE session encryption (that's
        // handled by BleHandshakeSession's ephemeral DH).
        let our_id = self.identity.signing_public_key();
        let (id_lo, id_hi) =
            super::key_order::sorted_pair(our_id.as_slice(), remote.identity_key.as_slice());
        let mut relay_info = b"vauchi-ble-relay-key-v1".to_vec();
        relay_info.extend_from_slice(id_lo);
        relay_info.extend_from_slice(id_hi);
        let dh_bytes = self.our_x3dh.diffie_hellman(&remote.exchange_key)?;
        let relay_derived = HKDF::derive_key(None, &*dh_bytes, &relay_info);
        let relay_key = crate::crypto::SymmetricKey::from_bytes(*relay_derived);

        // Retain the peer's X25519 exchange key so the post-Complete ratchet
        // setup keys off it (initiator side), consistent with the relay_key DH.
        self.their_exchange_key = Some(remote.exchange_key);

        let mut contact = Contact::from_exchange_full(
            remote.identity_key,
            their_card,
            relay_key,
            self.proximity_confidence,
            self.transport,
            0,
        );
        contact.set_relay_url(self.their_relay_url.take());

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

        // Retain the peer's X25519 exchange key for post-Complete ratchet
        // setup. The X3DH secret below is derived against this key, so the
        // Double Ratchet must be too (initiator side) — not the identity key.
        self.their_exchange_key = Some(their_exchange_key);

        // Symmetric DH: both sides have fresh ephemeral keys.
        // DH(our_secret × their_exchange_key) — both sides compute the same shared secret.
        // HKDF info binds all four public keys into the derivation (transcript binding),
        // preventing identity misbinding attacks. Keys are sorted lexicographically so
        // both sides compute identical info regardless of who is "Alice" vs "Bob".
        let shared_bytes = self.our_x3dh.diffie_hellman(&their_exchange_key)?;
        let our_id = self.identity.signing_public_key();
        let our_eph = self.our_x3dh.public_key();
        let (id_lo, id_hi) =
            super::key_order::sorted_pair(our_id.as_slice(), their_public_key.as_slice());
        let (eph_lo, eph_hi) =
            super::key_order::sorted_pair(our_eph.as_slice(), their_exchange_key.as_slice());
        let mut info = b"vauchi-x3dh-symmetric-v2".to_vec();
        info.extend_from_slice(id_lo);
        info.extend_from_slice(id_hi);
        info.extend_from_slice(eph_lo);
        info.extend_from_slice(eph_hi);
        let derived = HKDF::derive_key(None, &*shared_bytes, &info);
        let shared_key = crate::crypto::SymmetricKey::from_bytes(*derived);

        // Derive reciprocity confirmation tokens (design spec §2) via the
        // shared, transport-agnostic primitive so BLE / multi-stage derive
        // identically. Asymmetric: each side's token binds one identity key.
        let (our_confirm, their_confirm) = super::reciprocity_tokens::derive_confirmation_tokens(
            &shared_bytes[..],
            our_id.as_slice(),
            their_public_key.as_slice(),
        );
        self.our_confirmation_token = Some(our_confirm);
        self.expected_their_token = Some(their_confirm);

        // Derive confirmation escrow keys (design spec §3.5).
        let escrow_role = if super::key_order::is_initiator(our_id, &their_public_key) {
            super::escrow::EscrowRole::Initiator
        } else {
            super::escrow::EscrowRole::Responder
        };
        let confirm_escrow =
            super::confirmation_escrow::ConfirmationEscrowKeys::derive(&*shared_bytes, escrow_role);
        self.confirmation_gate_hash = Some(confirm_escrow.gate_hash);
        self.confirmation_our_slot = Some(confirm_escrow.our_slot);
        self.confirmation_their_slot = Some(confirm_escrow.their_slot);

        // USB/direct transport ships our card encrypted under the agreed key as
        // the second wire leg (the peer decrypts with the same shared key). Built
        // here, before `shared_key` moves into the state below.
        let usb_card_command = if self.transport == ExchangeTransport::Usb {
            Some(self.build_direct_send_card(&shared_key)?)
        } else {
            None
        };

        self.state = ExchangeState::AwaitingCardExchange {
            their_public_key,
            shared_key,
        };

        self.debug_event(ExchangeDebugEvent::KeyAgreementCompleted);

        // AU-2: Auto-invoke proximity check after key agreement. NFC and USB are
        // exempt: the physical tap / cable IS the proximity proof.
        if matches!(
            self.transport,
            ExchangeTransport::Nfc | ExchangeTransport::Usb
        ) {
            self.proximity_confidence = ProximityConfidence::High;
        } else {
            // ADR-031: Emit audio commands for async proximity verification.
            // The frontend handles audio I/O and reports AudioResponseReceived.
            // If no audio challenges are available (no QR scanned), fall back
            // to the synchronous verifier (ManualConfirmation etc.).
            self.emit_proximity_commands();
        }

        if let Some(cmd) = usb_card_command {
            self.emit_command(cmd);
        }

        Ok(())
    }

    fn handle_complete_exchange(
        &mut self,
        their_card: ContactCard,
    ) -> Result<Contact, ExchangeError> {
        // Use `Failed` as the placeholder instead of `Idle` so that if anything
        // between here and the `Complete` assignment panics, the session lands in
        // a terminal state rather than a misleadingly resumable one.
        let (their_public_key, shared_key) = match std::mem::replace(
            &mut self.state,
            ExchangeState::Failed {
                error: ExchangeError::Interrupted,
            },
        ) {
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
            0,
        );

        // Set relay metadata learned from their QR code
        contact.set_relay_url(self.their_relay_url.take());

        contact.set_trust_metrics(Some(self.build_trust_metrics()));

        self.state = ExchangeState::Complete {
            contact: Box::new(contact.clone()),
        };
        self.debug_event(ExchangeDebugEvent::ExchangeCompleted);

        Ok(contact)
    }

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
            self.clock.unix_seconds(),
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

        if qr.is_expired(self.clock.unix_seconds()) {
            return Err(ExchangeError::QRExpired);
        }
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *qr.public_key();
        let their_exchange_key = *qr.exchange_key();

        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        // AU-3: Store their audio challenge for session-bound proximity verification
        self.their_audio_challenge = Some(*qr.audio_challenge());
        // Store their display name from the QR code
        self.their_display_name = Some(qr.display_name().to_string());
        // Store their relay metadata for per-contact routing
        self.their_relay_url = qr.relay_url().map(String::from);

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

    fn handle_direct_payload_received(
        &mut self,
        their_payload: String,
    ) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Usb {
            return Err(ExchangeError::InvalidState(
                "DirectPayloadReceived requires Usb transport".into(),
            ));
        }
        if !matches!(self.state, ExchangeState::AwaitingDirectPayload { .. }) {
            return Err(ExchangeError::InvalidState(
                "Can only receive direct payload from AwaitingDirectPayload state".into(),
            ));
        }

        // Parse their payload (same format as QR data string)
        let qr = ExchangeQR::from_data_string(&their_payload)?;

        if qr.is_expired(self.clock.unix_seconds()) {
            return Err(ExchangeError::QRExpired);
        }
        if !qr.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        // Defense in depth: track consumed payload hashes to detect replays
        let payload_hash = {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            h.update(their_payload.as_bytes());
            let result = h.finalize();
            let mut hash = [0u8; 32];
            hash.copy_from_slice(&result);
            hash
        };
        self.check_qr_reuse(&payload_hash)?;

        let their_public_key = *qr.public_key();
        let their_exchange_key = *qr.exchange_key();

        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        // Store peer metadata
        self.their_audio_challenge = Some(*qr.audio_challenge());
        self.their_display_name = Some(qr.display_name().to_string());
        self.their_relay_url = qr.relay_url().map(String::from);

        // USB provides physical proximity — skip straight to key agreement
        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
    }

    /// Derive the USB card-exchange AEAD key from the agreed `shared_key`
    /// (ADR-007 domain separation; ADR-019 XChaCha20-Poly1305 at use sites).
    fn usb_card_key(shared_key: &crate::crypto::SymmetricKey) -> crate::crypto::SymmetricKey {
        let derived = HKDF::derive_key(None, shared_key.as_bytes(), USB_CARD_EXCHANGE_INFO);
        crate::crypto::SymmetricKey::from_bytes(*derived)
    }

    /// Build the `DirectSendCard` command — our card serialized + AEAD-encrypted
    /// under the USB card key.
    fn build_direct_send_card(
        &self,
        shared_key: &crate::crypto::SymmetricKey,
    ) -> Result<Command, ExchangeError> {
        let card_key = Self::usb_card_key(shared_key);
        let plaintext =
            serde_json::to_vec(&self.our_card).map_err(|_| ExchangeError::SerializationFailed)?;
        let ciphertext = crate::crypto::encryption::encrypt(&card_key, &plaintext)
            .map_err(|_| ExchangeError::SerializationFailed)?;
        Ok(Command::DirectSendCard {
            ciphertext,
            is_initiator: self.usb_role == Some(UsbRole::Initiator),
        })
    }

    /// Handle the peer's encrypted card (USB second leg): decrypt under the
    /// shared card key, parse, and complete the exchange.
    fn handle_direct_card_received(&mut self, ciphertext: Vec<u8>) -> Result<(), ExchangeError> {
        if self.transport != ExchangeTransport::Usb {
            return Err(ExchangeError::InvalidState(
                "DirectCardReceived requires Usb transport".into(),
            ));
        }
        let shared_key = match &self.state {
            ExchangeState::AwaitingCardExchange { shared_key, .. } => shared_key.clone(),
            _ => {
                return Err(ExchangeError::InvalidState(
                    "DirectCardReceived requires AwaitingCardExchange state".into(),
                ));
            }
        };
        let card_key = Self::usb_card_key(&shared_key);
        let plaintext = crate::crypto::encryption::decrypt(&card_key, &ciphertext)
            .map_err(|_| ExchangeError::UsbDecryptionFailed)?;
        let their_card: ContactCard =
            serde_json::from_slice(&plaintext).map_err(|_| ExchangeError::SerializationFailed)?;
        self.handle_complete_exchange(their_card).map(|_| ())
    }

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

        if parsed.is_expired(self.clock.unix_seconds()) {
            return Err(ExchangeError::NfcExpired);
        }
        if !parsed.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *parsed.identity_key();
        let their_exchange_key = *parsed.exchange_key();

        if their_public_key == *self.identity.signing_public_key() {
            return Err(ExchangeError::SelfExchange);
        }

        self.state = ExchangeState::AwaitingKeyAgreement {
            their_public_key,
            their_exchange_key,
        };
        Ok(())
    }

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

        if parsed.is_expired(self.clock.unix_seconds()) {
            return Err(ExchangeError::BleExpired);
        }
        if !parsed.verify_signature() {
            return Err(ExchangeError::InvalidSignature);
        }

        let their_public_key = *parsed.identity_key();
        let their_exchange_key = *parsed.exchange_key();

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

impl From<&str> for ExchangeError {
    fn from(s: &str) -> Self {
        ExchangeError::InvalidState(s.to_string())
    }
}
