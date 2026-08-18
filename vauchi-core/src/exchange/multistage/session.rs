// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage exchange session state machine.
//!
//! Manages the full 5-stage atomic QR exchange protocol:
//! `IDLE → ADVERTISING → DISCOVERED → TRANSFERRING → VERIFYING → CONFIRMING → COMPLETE → FINALIZED`
//!
//! Each session holds local card data, ephemeral X25519 keys for transport
//! encryption, an Ed25519 identity keypair, and a commitment scheme that
//! ensures atomicity (neither side can decrypt until both reveal keys are exchanged).
//!
//! Resilience features:
//! - Wall-clock timeouts with progress extension (not tick-based)
//! - COMBO QR (VRFY+CONF+RDYY) display in Complete state for single-scan finalization
//! - Adaptive QR display durations per stage
//! - Graduated retry: Complete → RetryReady → Failed (one auto-retry)
//! - FAIL QR type for immediate peer abort notification
//! - Scan acknowledgment via RDYY payload

use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::monotonic::{MonotonicClock, SystemMonotonicClock};

use chacha20poly1305::ChaCha20Poly1305;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use rand_core::OsRng;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey as X25519Public, StaticSecret as X25519Secret};
use zeroize::Zeroize;

use super::accel_envelope;
use super::chunker::{Chunker, ReassemblyBuffer};
use super::commitment::Commitment;
use super::qr_codec::{self, StageQr};
use super::types::{
    AccelerometerProximityState, AudioProximityState, ChunkBitmap, ProtocolState, QrPayload,
};

use crate::crypto::x3dh::X3DHKeyPair;
use crate::crypto::{DoubleRatchetState, SymmetricKey};
use crate::exchange::{key_order, ratchet_bootstrap};

/// Maximum raw payload bytes per chunk (before transport encryption overhead).
/// Transport encryption adds 12 (nonce) + 16 (Poly1305 tag) = 28 bytes overhead.
///
/// V4-optimized: 80 bytes → ~108 bytes encrypted → ~162 base45 chars.
/// With ~44 chars DATA header → ~206 total chars → fits V10 QR at ECC-M (213 chars).
/// Produces small, trivially decodable QR codes at 240p in ~9ms.
const CHUNK_PAYLOAD_SIZE: usize = 80;

/// HKDF info string for transport key derivation.
const HKDF_INFO: &[u8] = b"vauchi-multistage-v1";

/// Wall-clock timeout for the RDYY phase (Complete + RetryReady).
/// From device testing: Samsung S7 finalizes ~18s after Pixel.
/// 30s base gives comfortable margin for slow devices.
const RDYY_BASE_TIMEOUT: Duration = Duration::from_secs(30);

/// How much extra time is granted when the peer shows progress (e.g. scan detected).
const RDYY_PROGRESS_EXTENSION: Duration = Duration::from_secs(10);

/// Absolute maximum time in the RDYY phase (prevents infinite extension).
const RDYY_MAX_TIMEOUT: Duration = Duration::from_secs(45);

/// How long to broadcast FAIL QR after entering Failed state.
const FAIL_BROADCAST_DURATION: Duration = Duration::from_secs(5);

/// How long to continue broadcasting RDYY after Finalized (grace period for peer).
/// MUST be >= RDYY_MAX_TIMEOUT so the fast device never stops before the slow
/// device's timeout expires. The user sees "Contact exchanged!" immediately
/// (save on Finalized) while QRs continue in the background.
const FINALIZED_GRACE_DURATION: Duration = Duration::from_secs(60);

/// How long after `set_audio_proximity(Listening)` to wait for an
/// audio response before the cycle thread transitions the inner
/// state to Failed (Phase 1.C.7 of the Hover graduation plan). Mirror
/// of `MobileMultiStageSession::AUDIO_LISTEN_TIMEOUT_MS` — the
/// platform-layer constant scopes the adapter-side listen window;
/// this core-side budget is the defensive backstop in case the
/// adapter is silent (ADR-031: core does not trust hardware to
/// honour its `timeout_ms` honestly). The check is invoked by the
/// cycle thread via [`MultiStageSession::check_and_apply_audio_timeout`].
const AUDIO_LISTEN_TIMEOUT: Duration = Duration::from_secs(5);

/// How long after `set_accel_proximity(Listening)` to wait for the shake
/// to cross-correlate before transitioning to Failed (TapHoverShake P2.A).
/// Longer than `AUDIO_LISTEN_TIMEOUT` because the accel window must cover
/// the full `AccelerometerConfig::recording_duration_ms` motion recording
/// (3s) *plus* the peer-envelope round-trip over transport, where audio's
/// chirp-and-listen completes in a single acoustic exchange. Same defensive
/// backstop role (ADR-031): core enforces its own budget so the Listening
/// state cannot wedge if the platform adapter is silent. Checked by
/// [`MultiStageSession::check_and_apply_accel_timeout`].
const ACCEL_LISTEN_TIMEOUT: Duration = Duration::from_secs(8);

/// Base display durations per QR type (jitter added at runtime).
/// Tuned for <5s total exchange on typical hardware.
/// Shorter = more scan opportunities per second = faster convergence.
const DISPLAY_MS_INIT: u32 = 400;
/// A frame must stay up long enough for the peer to *capture* it, which is
/// not the same as decoding it. The 100 ms this replaced was validated on
/// Pixel 3a ↔ Galaxy S7 with the ML Kit scanner at 240p (`56c3cff1`); the
/// analyzer has since moved to rxing at 480p, whose per-frame capture is
/// correspondingly slower, and dim-room auto-exposure alone can integrate
/// longer than 100 ms.
///
/// Device-measured 2026-08-18 (Pixel 3a ↔ iPhone SE), counting the scanner's
/// own decodes of the peer's DATA frames over comparable windows:
///
/// | dwell  | DATA frames decoded | transfer reached |
/// |--------|---------------------|------------------|
/// | 100 ms | 0                   | nothing          |
/// | 300 ms | 1048                | `recv=2/3`       |
/// | 400 ms | 105                 | `recv=1/3`       |
///
/// 300 ms is the empirical optimum: long enough to be captured at all, short
/// enough to keep the frame rate that gives the peer repeated attempts.
/// Raising it to `DISPLAY_MS_INIT` decoded *fewer* frames and got *less* far,
/// so DATA keeps its own value rather than being tied to INIT.
///
/// Note DATA is *not* the denser code, despite being longer: 152 bytes on the
/// wire against INI2's 139, yet both render to 41 modules. Byte length stops
/// tracking density once both payloads are base45, whose alphabet is exactly
/// the QR alphanumeric charset. The `error_correction` these frames declare
/// does not explain the gap either — no shell reads it (Android's
/// `generateQrBitmap` defaults every code to Medium, iOS never references the
/// field), so INIT and DATA render identically on device.
const DISPLAY_MS_DATA: u32 = 300;
const DISPLAY_MS_VRFY: u32 = 300;
const DISPLAY_MS_CONF: u32 = 300;
const DISPLAY_MS_RDYY: u32 = 400;
const DISPLAY_MS_FAIL: u32 = 400;

/// Add ±20% jitter to prevent synchronization lock between two devices.
/// When both devices cycle QRs at identical cadence, they can stay in phase
/// and always scan during the other's transition — missing every QR.
/// Jitter breaks the phase alignment.
fn jittered(base_ms: u32) -> u32 {
    let jitter_range = base_ms / 5; // ±20%
    let jitter: u32 = crate::crypto::random_bytes::<4>()
        .iter()
        .fold(0u32, |acc, &b| acc.wrapping_add(b as u32))
        % (jitter_range * 2 + 1);
    base_ms - jitter_range + jitter
}

/// Uniformly pick a slot below `bound`, drawn fresh for each displayed frame.
///
/// Choosing which frame to show from a position in a display cycle makes the
/// frame a function of that counter, and a camera samples on its own clock:
/// when the two periods share a factor the scanner lands on the same slot
/// forever and never sees the others. Device-proven — a scanner aliased to
/// the 4-slot Transferring cycle received the same chunk 692 times while the
/// two chunks it still needed were never once displayed to it
/// (`2026-08-18-hover-transfer-stalls-on-the-last-chunk`). Drawing each frame
/// independently leaves every slot reachable from every sampling phase.
fn random_below(bound: u32) -> u32 {
    u32::from_le_bytes(crate::crypto::random_bytes::<4>()) % bound.max(1)
}

/// Audio-proximity state transition error.
///
/// The session enforces a strict transition graph to prevent the
/// caller (the platform wrapper that orchestrates the audio
/// handshake) from skipping the verification gate. In particular,
/// `Confirmed` is only reachable from `Listening` — never directly
/// from `Pending` or `Failed` — because `Confirmed` is the
/// security claim that the two devices are physically close, and
/// that claim is only valid after an actual ultrasonic response
/// verified successfully (the `Listening` window is the
/// chirp-and-verify cycle).
///
/// Retry semantics (G1.3 of the Hover graduation problem record):
/// `Failed → Listening` is allowed so the user can re-attempt the
/// handshake without restarting the QR cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioStateError {
    /// The requested transition is not part of the state graph.
    /// Carries the from-state and to-state so the caller can log a
    /// diagnostic. Indicates a programming error in the orchestrator;
    /// in production the wrapper should surface this to the user as
    /// a generic proximity failure rather than panic.
    InvalidTransition {
        from: AudioProximityState,
        to: AudioProximityState,
    },
}

/// Error returned by [`MultiStageSession::set_accel_proximity`] when a
/// requested accelerometer-proximity transition is not part of the state
/// graph. The TapHoverShake-side mirror of [`AudioStateError`]; kept as a
/// distinct type (not a shared generic) so the carried `from`/`to` are
/// typed `AccelerometerProximityState` and a wrongly-wired
/// audio/accel transition is a compile error, not a runtime confusion.
///
/// Retry semantics mirror audio: `Failed → Listening` is allowed so the
/// user can re-attempt the shake without restarting the QR cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccelStateError {
    /// The requested transition is not part of the state graph. Carries
    /// the from-state and to-state for diagnostics. Indicates a
    /// programming error in the orchestrator; the wrapper should surface
    /// it as a generic proximity failure rather than panic.
    InvalidTransition {
        from: AccelerometerProximityState,
        to: AccelerometerProximityState,
    },
}

/// Reason a transport-encrypted DATA chunk was rejected.
///
/// Private to the module — exposed only as a counter via
/// [`MultiStageSession::transport_decrypt_failure_count`]. Distinguishing
/// the two variants publicly would let an attacker probe AEAD internals
/// via error-shape oracles, so the observable surface is a single count
/// rather than a per-reason histogram.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TransportDecryptError {
    /// Ciphertext shorter than nonce(12) + tag(16); malformed framing.
    CiphertextTooShort,
    /// AEAD verification failed: wrong key, tampered ciphertext, or
    /// replay against a different chunk index.
    AeadFailure,
}

/// Multi-stage exchange session managing the full 5-stage protocol.
///
/// The session progresses through states:
/// 1. **Idle** — created, not yet started
/// 2. **Advertising** — displaying INIT QR with our public keys and commitment hash
/// 3. **Discovered** — scanned peer's INIT, derived transport key (transient)
/// 4. **Transferring** — exchanging encrypted DATA chunks bidirectionally
/// 5. **Verifying** — all chunks exchanged, sending/receiving reveal keys
/// 6. **Confirming** — reveal key verified, sending/receiving confirmation
/// 7. **Complete** — both sides confirmed, peer data available
pub struct MultiStageSession {
    local_card: Vec<u8>,
    display_name: String,
    commitment: Commitment,
    session_id: [u8; 16],

    ephemeral_secret: Option<X25519Secret>,
    ephemeral_public: X25519Public,
    // Copy of the transport ephemeral secret, retained past `ephemeral_secret`'s
    // `take()` at transport-key derivation so the post-finalize Double Ratchet
    // bootstrap (responder role) can key off it — exactly as the unified
    // ExchangeSession retains `our_x3dh`. Keeping the ratchet root dependent on
    // this fresh ephemeral (not transport_key alone) preserves the property that
    // transport_key compromise — it is backed up + synced — does not reveal the
    // root. Zeroized on drop via `clear_sensitive`. See
    // `_private/docs/problems/2026-05-25-in-person-exchange-ratchet-broken`.
    ratchet_ephemeral: Option<X25519Secret>,

    // Peer data (populated after Stage 1)
    peer_ephemeral: Option<[u8; 32]>,
    peer_commitment_hash: Option<[u8; 32]>,
    peer_session_id: Option<[u8; 16]>,

    transport_key: Option<[u8; 32]>,

    // Outbound chunks (transport-encrypted commitment ciphertext)
    outbound_chunks: Vec<Vec<u8>>,
    outbound_total: u16,

    inbound_buffer: Option<ReassemblyBuffer>,
    inbound_bitmap: Option<ChunkBitmap>,
    peer_ack_bitmap: Option<ChunkBitmap>,
    peer_chunks_total: Option<u16>,
    // Count of DATA chunks rejected by the transport AEAD layer.
    // Site 1 of `2026-05-21-silent-failures-in-security-paths`: pre-
    // 2026-05-23 `transport_decrypt` returned `Option<Vec<u8>>` so
    // forgery probes, tag-mismatch corruption, and "chunk hasn't
    // arrived yet" were indistinguishable from outside the session.
    transport_decrypt_failures: u32,

    peer_reveal_key: Option<[u8; 32]>,

    state: ProtocolState,

    // Received peer data (populated at Complete)
    received_data: Option<Vec<u8>>,

    // INIT QR cache (avoid regenerating)
    init_qr_cache: Option<String>,

    // Outbound chunks still to show this pass, in shuffled order (popped from
    // the back). Rebuilt from the unacked set whenever a pass drains.
    pending_chunk_order: Vec<u16>,

    // Display cycle counter — every Nth cycle, re-show INIT so a slower
    // peer still in Advertising can discover us.
    display_cycle: u32,

    // Wall-clock timestamps for timeout management.
    phase_entered_at: Option<Instant>,
    last_progress_at: Option<Instant>,

    // Track whether we've already used the auto-retry.
    retry_used: bool,

    // When Failed, continue broadcasting FAIL QR until this deadline.
    fail_broadcast_until: Option<Instant>,

    // Our relay metadata (included in our INIT QR)
    our_relay_url: Option<String>,

    // Peer's relay metadata (extracted from their INIT QR)
    peer_relay_url: Option<String>,

    // Audio-proximity verification state (Hover-only).
    //
    // Glance never transitions this — the mode doesn't run an
    // ultrasonic handshake. Hover sub-flow drives it through
    // `Pending → Listening → Confirmed` on success or
    // `Pending → Listening → Failed` on timeout. Phase 1.C.3a
    // foothold: the field + getter land first so the engine-side
    // setter shipped in Phase 1.C.2 (vauchi/core!794) has a
    // session-side counterpart to bridge into via the listener
    // callback (1.C.3c) once the state machine (1.C.3b) emits
    // transitions.
    //
    // Lives in vauchi-core per the design pass
    // (_private/docs/problems/2026-04-28-multi-stage-engine-hover-ultrasonic/investigation.md
    // Option B) so the session (protocol producer) and the
    // MultiStageExchangeEngine (renderer consumer) share one
    // enum definition.
    audio_proximity: AudioProximityState,

    /// `Some(t)` while `audio_proximity == Listening` — `t` is the
    /// monotonic `Instant` at which the listen window opened.
    /// `None` outside Listening. Read by
    /// [`Self::check_and_apply_audio_timeout`] to enforce the
    /// `AUDIO_LISTEN_TIMEOUT` budget on the cycle thread. The
    /// timestamp source is `self.monotonic.now()` inside
    /// [`Self::set_audio_proximity`]; tests inject a
    /// `FakeMonotonicClock` via [`Self::with_monotonic`] and advance it
    /// (matches the existing `phase_entered_at` style in this
    /// session for ProtocolState transitions).
    audio_listening_started_at: Option<Instant>,

    /// TapHoverShake accelerometer-proximity state — the second parallel
    /// proximity signal alongside `audio_proximity`. `Pending` for Glance
    /// and Hover (never transitioned); TapHoverShake drives it through the
    /// shake-correlation states. See [`AccelerometerProximityState`].
    accel_proximity: AccelerometerProximityState,
    /// `Some(t)` while `accel_proximity == Listening` — `t` is the
    /// monotonic `Instant` at which the capture window opened, read by
    /// [`Self::check_and_apply_accel_timeout`] to enforce the
    /// `ACCEL_LISTEN_TIMEOUT` budget. Stamped from `self.monotonic.now()`
    /// in [`Self::set_accel_proximity`], same clock domain as the audio
    /// window — `None` outside Listening.
    accel_recording_started_at: Option<Instant>,
    /// Locally-recorded accelerometer magnitude envelope for the TapHoverShake
    /// shake co-location signal. Accumulated via
    /// [`Self::record_accel_envelope_samples`] while `accel_proximity ==
    /// Listening`; sealed into the SHAK QR on emit (F2 sender-AAD binding) and
    /// cross-correlated against the peer's on receive. Dropped after correlation
    /// and in `clear_sensitive` (F7) — transient proximity proof, never card data.
    accel_local_envelope: Vec<f32>,
    /// Explicit-monotonic-time seam (Phase 1 / Task 1.1b). Source for
    /// every `Instant` this session stamps (`phase_entered_at`,
    /// `last_progress_at`, `fail_broadcast_until`,
    /// `audio_listening_started_at`) and the RDYY/finalized/FAIL
    /// timeout comparisons. Defaults to `SystemMonotonicClock::shared()`;
    /// inject via [`Self::with_monotonic`] for deterministic timeout
    /// tests. Note `check_and_apply_audio_timeout` retains its explicit
    /// `now: Instant` parameter for cycle-thread callers.
    monotonic: Arc<dyn MonotonicClock>,
}

/// Errors building the post-finalize Double Ratchet for a multi-stage exchange.
#[derive(Debug, thiserror::Error)]
pub enum RatchetSetupError {
    #[error("multi-stage session has no transport key (not finalized)")]
    NoTransportKey,
    #[error("multi-stage session has no peer ephemeral key")]
    NoPeerEphemeral,
    #[error("multi-stage session ephemeral was not retained")]
    NoEphemeral,
    #[error("ratchet initialization failed: {0}")]
    RatchetInit(String),
}

impl MultiStageSession {
    /// Create a new session for exchanging the given contact card data.
    pub fn new(local_card: Vec<u8>) -> Self {
        Self::new_with_relay(local_card, None)
    }

    /// Create a new session with optional relay metadata.
    ///
    /// The relay URL will be included in our INIT QR so the peer can
    /// route future messages to our relay.
    pub fn new_with_relay(local_card: Vec<u8>, relay_url: Option<String>) -> Self {
        let session_id: [u8; 16] = crate::crypto::random_bytes();

        let ephemeral_secret = X25519Secret::random_from_rng(OsRng);
        let ephemeral_public = X25519Public::from(&ephemeral_secret);

        // Create commitment from local card, binding our relay metadata (T1.7).
        //
        // `Commitment::create_with_context` is fallible at the API level
        // (post-2026-05-21 ADR-019 XChaCha20 migration), but the only
        // failure path is `XChaCha20Poly1305::encrypt` returning Err —
        // which AEAD encryption of arbitrary plaintext with a fresh
        // random key + random 24-byte nonce cannot do in practice. The
        // panic here asserts that invariant. Propagating up through
        // `MultiStageSession::new` is a separate refactor tracked as a
        // follow-up to `2026-05-21-adr-019-commitment-xchacha-consistency`.
        let commitment_context = Self::build_commitment_context(relay_url.as_deref());
        let commitment = Commitment::create_with_context(&local_card, &commitment_context)
            .expect("XChaCha20Poly1305 encryption of fresh plaintext cannot fail");

        MultiStageSession {
            local_card,
            display_name: "Vauchi User".to_string(),
            commitment,
            session_id,
            ephemeral_secret: Some(ephemeral_secret),
            ratchet_ephemeral: None,
            ephemeral_public,
            peer_ephemeral: None,
            peer_commitment_hash: None,
            peer_session_id: None,
            transport_key: None,
            outbound_chunks: Vec::new(),
            outbound_total: 0,
            inbound_buffer: None,
            inbound_bitmap: None,
            peer_ack_bitmap: None,
            peer_chunks_total: None,
            transport_decrypt_failures: 0,
            peer_reveal_key: None,
            state: ProtocolState::Idle,
            received_data: None,
            init_qr_cache: None,
            pending_chunk_order: Vec::new(),
            display_cycle: 0,
            phase_entered_at: None,
            last_progress_at: None,
            retry_used: false,
            fail_broadcast_until: None,
            our_relay_url: relay_url,
            peer_relay_url: None,
            audio_proximity: AudioProximityState::Pending,
            audio_listening_started_at: None,
            accel_proximity: AccelerometerProximityState::Pending,
            accel_recording_started_at: None,
            accel_local_envelope: Vec::new(),
            monotonic: SystemMonotonicClock::shared(),
        }
    }

    /// Replace the [`MonotonicClock`] driving this session's timeout and
    /// timestamp logic. Default is [`SystemMonotonicClock::shared`];
    /// inject a `FakeMonotonicClock` for deterministic RDYY/finalized/
    /// audio-timeout tests.
    #[must_use]
    /// Borrow this session's monotonic clock so the platform cycle-thread
    /// driver feeds the same clock into `check_and_apply_audio_timeout`,
    /// keeping the recorded start and the timeout `now` in one domain.
    pub fn monotonic(&self) -> &Arc<dyn MonotonicClock> {
        &self.monotonic
    }

    pub fn with_monotonic(mut self, monotonic: Arc<dyn MonotonicClock>) -> Self {
        self.monotonic = monotonic;
        self
    }

    /// Returns the current protocol state.
    pub fn get_state(&self) -> ProtocolState {
        self.state.clone()
    }

    /// Returns the locally-generated 16-byte session ID. Included in
    /// our INIT QR so the peer learns it during Stage 1; serves as the
    /// audio-handshake challenge in Phase 1.C.3e (Hover graduation).
    ///
    /// Not a secret — the session_id is broadcast in the INIT QR. The
    /// audio handshake's security primitive is that the audio channel
    /// itself requires physical proximity to receive; the bytes being
    /// broadcast in QR don't reduce that.
    ///
    /// Phase 1.C.4 may introduce a dedicated `audio_challenge` field
    /// on the QR payload (orthogonal to session_id) so the session_id
    /// stays inert and a per-handshake nonce drives the FSK challenge.
    /// Today the two coincide.
    pub fn session_id(&self) -> [u8; 16] {
        self.session_id
    }

    /// Returns the peer's session ID, populated during Stage 1 from
    /// the peer's INIT QR. `None` before the peer has been
    /// discovered. Used by the Phase 1.C.3e Hover audio handshake to
    /// verify the decoded FSK response: a valid Confirmed transition
    /// requires the received samples to decode to these exact bytes
    /// (the peer is broadcasting their own session_id via ultrasonic;
    /// if the audio channel carried it intact to our mic, the peer is
    /// physically close).
    pub fn peer_session_id(&self) -> Option<[u8; 16]> {
        self.peer_session_id
    }

    /// Verify a decoded FSK audio response against the peer's
    /// session_id using constant-time equality. Returns `Some(true)`
    /// on match, `Some(false)` on mismatch or wrong length, and
    /// `None` if `peer_session_id` is not yet set (Stage 1 incomplete).
    ///
    /// Security primitive for Phase 1.C.3e-iv Hover handshake
    /// verification: keeps the timing-side-channel resistance inside
    /// vauchi-core (which already depends on `subtle`) rather than
    /// pushing that crypto-adjacent code into vauchi-platform.
    pub fn verify_audio_response(&self, decoded: &[u8]) -> Option<bool> {
        use subtle::ConstantTimeEq;
        let peer = self.peer_session_id?;
        if decoded.len() != 16 {
            return Some(false);
        }
        let arr: [u8; 16] = decoded.try_into().ok()?;
        Some(bool::from(arr.ct_eq(&peer)))
    }

    /// Returns the received peer data if the exchange is finalized.
    ///
    /// Data is only available in the `Finalized` state — both sides must
    /// have exchanged READY QRs to confirm mutual completion. This ensures
    /// atomicity: neither side persists a contact unless both succeeded.
    pub fn get_received_data(&self) -> Option<Vec<u8>> {
        if matches!(self.state, ProtocolState::Finalized) {
            self.received_data.clone()
        } else {
            None
        }
    }

    /// Returns the transport key derived during the ECDH key exchange.
    ///
    /// Available after the Discovered state (once both sides have exchanged
    /// ephemeral public keys). Used by the mobile layer to derive the shared
    /// secret for the double ratchet after exchange completion.
    pub fn get_transport_key(&self) -> Option<[u8; 32]> {
        self.transport_key
    }

    /// Builds the role-correct Double Ratchet for a finalized multi-stage exchange.
    ///
    /// Role decision and role-correct keying are delegated to
    /// [`crate::exchange::ratchet_bootstrap::bootstrap_exchange_ratchet`]
    /// (shared with `ExchangeSession`). Here the initiator's peer key is
    /// the peer's transport ephemeral (`peer_ephemeral`) and the
    /// responder's own key is the retained transport ephemeral
    /// (`ratchet_ephemeral`). `transport_key` is the root seed.
    ///
    /// Keeping the root dependent on a fresh ephemeral DH (not `transport_key`
    /// alone) preserves the property that `transport_key` compromise -- it is
    /// backed up and synced -- does not by itself reveal the ratchet root. Pure
    /// crypto; persistence stays with the caller.
    pub fn build_exchange_ratchet(
        &self,
        our_identity: &[u8; 32],
        their_identity: &[u8; 32],
    ) -> Result<(DoubleRatchetState, bool), RatchetSetupError> {
        let transport_key = self
            .transport_key
            .ok_or(RatchetSetupError::NoTransportKey)?;
        let shared = SymmetricKey::from_bytes(transport_key);
        let our_ephemeral = self
            .ratchet_ephemeral
            .as_ref()
            .map(|secret| X3DHKeyPair::from_bytes(secret.to_bytes()));
        ratchet_bootstrap::bootstrap_exchange_ratchet(
            &shared,
            our_identity,
            their_identity,
            self.peer_ephemeral,
            our_ephemeral,
        )
        .map_err(|e| match e {
            ratchet_bootstrap::RatchetBootstrapError::MissingPeerEphemeral => {
                RatchetSetupError::NoPeerEphemeral
            }
            ratchet_bootstrap::RatchetBootstrapError::MissingOurEphemeral => {
                RatchetSetupError::NoEphemeral
            }
            ratchet_bootstrap::RatchetBootstrapError::Init(msg) => {
                RatchetSetupError::RatchetInit(msg)
            }
        })
    }

    /// Derive the reciprocity confirmation token pair from the agreed transport
    /// key + both identity keys (design spec §2), via the shared transport-
    /// agnostic primitive. `None` until key agreement has produced a transport
    /// key. Pure (like `build_exchange_ratchet`): multi-stage does not store the
    /// identities, so the caller supplies them. Both peers derive a cross-
    /// matching pair, so the shared `ReciprocityConfirmer` can confirm over the
    /// multi-stage session's own channel (relay-free).
    pub fn confirmation_tokens(
        &self,
        our_identity: &[u8; 32],
        their_identity: &[u8; 32],
    ) -> Option<super::super::reciprocity_tokens::ConfirmationTokenPair> {
        let transport_key = self.transport_key?;
        Some(
            super::super::reciprocity_tokens::derive_confirmation_tokens(
                &transport_key,
                our_identity,
                their_identity,
            ),
        )
    }

    /// Returns the peer's relay URL (available after INIT exchange).
    pub fn peer_relay_url(&self) -> Option<&str> {
        self.peer_relay_url.as_deref()
    }

    /// Returns the current audio-proximity state. Hover-only — Glance
    /// callers see `Pending` for the lifetime of the session because
    /// Glance never runs the ultrasonic handshake. Phase 1.C.3b
    /// added the state machine driven by [`Self::set_audio_proximity`].
    pub fn audio_proximity(&self) -> AudioProximityState {
        self.audio_proximity
    }

    /// Drive the audio-proximity state machine.
    ///
    /// Called by the platform wrapper (Phase 1.C.3c) after each
    /// transition the orchestrator decides: `Listening` when the
    /// chirp emit + listen window opens, `Confirmed` when the peer's
    /// response verified, `Failed` on timeout or verification
    /// mismatch.
    ///
    /// **Transition graph (security gate)**:
    ///
    /// ```text
    ///   Pending ──► Listening ──► Confirmed   (success)
    ///                       └──► Failed      (timeout / mismatch)
    ///                                ↑
    ///                              Listening (retry per G1.3)
    /// ```
    ///
    /// `Confirmed` is reachable **only** from `Listening` so that
    /// the security claim ("devices are physically close") cannot
    /// be set without an actual verified ultrasonic exchange.
    /// `Failed → Listening` is allowed so the user can re-run the
    /// handshake without restarting the QR cycle (Hover problem
    /// record G1.3).
    ///
    /// Returns `Err(InvalidTransition { … })` for any transition
    /// not in the graph. The caller is expected to log + surface
    /// a generic proximity-failure UX rather than panic.
    pub fn set_audio_proximity(
        &mut self,
        new_state: AudioProximityState,
    ) -> Result<(), AudioStateError> {
        if !Self::audio_transition_allowed(self.audio_proximity, new_state) {
            return Err(AudioStateError::InvalidTransition {
                from: self.audio_proximity,
                to: new_state,
            });
        }
        self.audio_proximity = new_state;
        // Track the listen-window entry for
        // `check_and_apply_audio_timeout` (Phase 1.C.7). The window
        // is open exactly while `audio_proximity == Listening` —
        // record on entry, clear on exit. `self.monotonic.now()` is the
        // monotonic source (Phase 1 / Task 1.1b seam) — the same clock
        // `check_and_apply_audio_timeout`'s caller must read, so the
        // recorded start and the timeout `now` share a clock domain.
        self.audio_listening_started_at = match new_state {
            AudioProximityState::Listening => Some(self.monotonic.now()),
            _ => None,
        };
        Ok(())
    }

    /// Enforce the audio-listen timeout budget.
    ///
    /// Invoked by the cycle thread (and by tests) with the current
    /// `Instant`. Returns:
    ///
    /// - `Ok(false)` and leaves state untouched when not in Listening
    ///   or when the budget hasn't elapsed yet.
    /// - `Ok(true)` and transitions Listening → Failed when the
    ///   budget has elapsed.
    /// - `Err(AudioStateError::InvalidTransition)` is structurally
    ///   unreachable — Listening → Failed is in the allowed graph —
    ///   but the result type matches `set_audio_proximity` for
    ///   caller symmetry.
    ///
    /// Defensive backstop per ADR-031: the platform-layer
    /// `Command::AudioListenForResponse { timeout_ms }` tells the
    /// audio adapter how long to listen, but the adapter may be
    /// silent on timeout (no "no peer audio detected" event in the
    /// hardware-event vocabulary). Core enforces its own budget so
    /// the Listening state cannot wedge.
    ///
    /// The matching test scenarios are in
    /// `vauchi-core/tests/it/multistage_session_tests.rs`
    /// (`audio_timeout_*` block — Phase 1.C.6 RED).
    pub fn check_and_apply_audio_timeout(&mut self, now: Instant) -> Result<bool, AudioStateError> {
        let started = match (self.audio_proximity, self.audio_listening_started_at) {
            (AudioProximityState::Listening, Some(t)) => t,
            // Not in Listening, or in Listening but tracker missing
            // (shouldn't happen via `set_audio_proximity`; defensive
            // no-op so a future refactor that forgets to set the
            // timestamp fails loud via the test suite, not at
            // runtime).
            _ => return Ok(false),
        };
        if now.saturating_duration_since(started) < AUDIO_LISTEN_TIMEOUT {
            return Ok(false);
        }
        self.set_audio_proximity(AudioProximityState::Failed)?;
        Ok(true)
    }

    /// Returns `true` iff the transition is permitted by the audio
    /// state graph. Pure function — exposed for tests and so the
    /// orchestrator can preflight before invoking
    /// [`Self::set_audio_proximity`].
    pub(crate) fn audio_transition_allowed(
        from: AudioProximityState,
        to: AudioProximityState,
    ) -> bool {
        use AudioProximityState::{Confirmed, Failed, Listening, Pending};
        matches!(
            (from, to),
            (Pending, Listening)
                | (Listening, Confirmed)
                | (Listening, Failed)
                | (Failed, Listening)
        )
    }

    /// Returns the current accelerometer-proximity state. TapHoverShake-only
    /// — Glance and Hover callers see `Pending` for the session lifetime
    /// because neither runs the shake correlation.
    pub fn accel_proximity(&self) -> AccelerometerProximityState {
        self.accel_proximity
    }

    /// Drive the accelerometer-proximity state machine.
    ///
    /// The TapHoverShake mirror of [`Self::set_audio_proximity`], with the
    /// identical transition graph and security gate: `Confirmed` is
    /// reachable **only** from `Listening`, so the "devices shook together"
    /// claim cannot be set without an actual recording + cross-correlation.
    /// `Failed -> Listening` permits a retry without restarting the QR cycle.
    ///
    /// ```text
    ///   Pending --> Listening --> Confirmed   (cross-correlation >= threshold)
    ///                       \--> Failed       (timeout / mismatch)
    ///                                ^
    ///                              Listening  (retry)
    /// ```
    ///
    /// Returns `Err(InvalidTransition { .. })` for any transition not in the
    /// graph; the caller logs + surfaces a generic proximity-failure UX.
    pub fn set_accel_proximity(
        &mut self,
        new_state: AccelerometerProximityState,
    ) -> Result<(), AccelStateError> {
        if !Self::accel_transition_allowed(self.accel_proximity, new_state) {
            return Err(AccelStateError::InvalidTransition {
                from: self.accel_proximity,
                to: new_state,
            });
        }
        self.accel_proximity = new_state;
        // Open the capture window on entry to Listening, clear on exit --
        // same clock domain as the audio window so the recorded start and
        // the timeout `now` share `self.monotonic`.
        self.accel_recording_started_at = match new_state {
            AccelerometerProximityState::Listening => {
                // Fresh capture window — discard any envelope from a prior attempt.
                self.accel_local_envelope.clear();
                Some(self.monotonic.now())
            }
            _ => None,
        };
        Ok(())
    }

    /// Enforce the accelerometer-capture timeout budget. The TapHoverShake
    /// mirror of [`Self::check_and_apply_audio_timeout`]:
    ///
    /// - `Ok(false)` and leaves state untouched when not in Listening or
    ///   when the `ACCEL_LISTEN_TIMEOUT` budget has not elapsed.
    /// - `Ok(true)` and transitions Listening -> Failed once it has.
    /// - `Err(AccelStateError::InvalidTransition)` is structurally
    ///   unreachable (Listening -> Failed is in the graph) but the result
    ///   type matches `set_accel_proximity` for caller symmetry.
    ///
    /// Defensive backstop per ADR-031: `Command::AccelerometerStart` gives
    /// the adapter no timeout, and `AccelerometerData` carries no "no peer
    /// motion" signal, so core enforces its own budget to keep the
    /// Listening state from wedging.
    pub fn check_and_apply_accel_timeout(&mut self, now: Instant) -> Result<bool, AccelStateError> {
        let started = match (self.accel_proximity, self.accel_recording_started_at) {
            (AccelerometerProximityState::Listening, Some(t)) => t,
            _ => return Ok(false),
        };
        if now.saturating_duration_since(started) < ACCEL_LISTEN_TIMEOUT {
            return Ok(false);
        }
        self.set_accel_proximity(AccelerometerProximityState::Failed)?;
        Ok(true)
    }

    /// Returns `true` iff the transition is permitted by the accelerometer
    /// state graph. Pure function -- exposed for tests and orchestrator
    /// preflight. Same graph as [`Self::audio_transition_allowed`].
    pub(crate) fn accel_transition_allowed(
        from: AccelerometerProximityState,
        to: AccelerometerProximityState,
    ) -> bool {
        use AccelerometerProximityState::{Confirmed, Failed, Listening, Pending};
        matches!(
            (from, to),
            (Pending, Listening)
                | (Listening, Confirmed)
                | (Listening, Failed)
                | (Failed, Listening)
        )
    }

    /// Append locally-captured accelerometer magnitude samples to the SHAK
    /// envelope. Samples outside the `Listening` capture window are ignored.
    ///
    /// The engine forwards `Event::AccelerometerData` here while the
    /// TapHoverShake shake stage is active (wired in the engine; ADR-031
    /// command/event). Glance/Hover never enter `Listening`, so they record
    /// nothing and never emit a SHAK.
    pub fn record_accel_envelope_samples(&mut self, samples: &[f32]) {
        if self.accel_proximity == AccelerometerProximityState::Listening {
            self.accel_local_envelope.extend_from_slice(samples);
        }
    }

    /// Number of samples in the local accelerometer envelope captured so far.
    /// The engine reads this as a shake-capture progress / readiness seam.
    pub fn accel_envelope_len(&self) -> usize {
        self.accel_local_envelope.len()
    }

    /// Build the SHAK QR carrying our AEAD-sealed accelerometer envelope, or
    /// `None` if the shake stage is not ready to transmit.
    ///
    /// F5 timing gate: `None` unless `transport_key` exists (post-VRFY DH),
    /// `accel_proximity == Listening`, and we have recorded samples. The
    /// envelope is sealed under our own `session_id` (F2 sender-AAD binding) so
    /// a peer that reflects it back fails AEAD at us.
    /// Build a finalization-stage QR, interleaving VRFY and CONF across the
    /// `display_cycle % 7` phases (VRFY on 0–2, CONF on 3–6; SHAK on 6 when
    /// available). Both VRFY (our reveal key) and CONF (our card hash) are
    /// always computable in the finalization states, and a peer may be in
    /// Verifying (needs our VRFY) or Confirming (needs our CONF) — so **both
    /// must be offered from BOTH states**. Offering CONF from Verifying is
    /// the fix for the device-proven Pixel↔Samsung Hover deadlock 2026-07-24:
    /// one peer lingered in Verifying emitting only VRFY while the other
    /// starved for CONF and timed out. A peer that receives a frame it is not
    /// yet ready for ignores it (`handle_confirm`/`handle_verify` no-op
    /// off-state). Deterministic (no RNG) so the exchange timing is
    /// reproducible. The cadence alone does not stop a scanner locking onto
    /// one phase — COMBO is what makes finalization robust to that, since any
    /// single COMBO decode advances the peer (see `random_below` for the
    /// DATA-phase instance where no such frame exists).
    fn build_finalization_qr(&self, include_shake: bool) -> QrPayload {
        let phase = self.display_cycle % 7;
        // SHAK carries the advisory accel envelope (TapHoverShake);
        // `build_shake_qr` returns None for Glance/Hover, falling through.
        if include_shake
            && phase == 6
            && let Some(shake_qr) = self.build_shake_qr()
        {
            return QrPayload {
                data: shake_qr,
                error_correction: "M".to_string(),
                display_duration_ms: jittered(DISPLAY_MS_CONF),
            };
        }
        if phase < 3 {
            QrPayload {
                data: qr_codec::format_verify_qr(&self.session_id, self.commitment.reveal_key()),
                error_correction: "M".to_string(),
                display_duration_ms: jittered(DISPLAY_MS_VRFY),
            }
        } else {
            let card_hash = self.compute_card_hash(&self.local_card);
            QrPayload {
                data: qr_codec::format_confirm_qr(&self.session_id, &card_hash),
                error_correction: "M".to_string(),
                display_duration_ms: jittered(DISPLAY_MS_CONF),
            }
        }
    }

    fn build_shake_qr(&self) -> Option<String> {
        if self.accel_proximity != AccelerometerProximityState::Listening
            || self.accel_local_envelope.is_empty()
        {
            return None;
        }
        let key = self.transport_key?;
        let sealed =
            accel_envelope::seal_envelope(&key, &self.session_id, &self.accel_local_envelope);
        Some(qr_codec::format_shake_qr(&self.session_id, &sealed))
    }

    /// Handle an inbound SHAK stage: open the peer's sealed envelope and
    /// cross-correlate it against our local recording to drive `accel_proximity`.
    ///
    /// Advisory only — returns the unchanged `ProtocolState` and never gates
    /// completion (security-review F8). Gates:
    /// - Acts only while `accel_proximity == Listening` (TapHoverShake) and we
    ///   have a local envelope — other modes record nothing to correlate.
    /// - F5: requires `transport_key`; a SHAK arriving before the key exists is
    ///   dropped (no buffering, no error).
    /// - F2: opens under `peer_session_id`, so a reflected own-envelope (sealed
    ///   under our sid) fails AEAD and is dropped, leaving `Listening` for the
    ///   timeout backstop to resolve to `Failed`.
    fn handle_shake(&mut self, sealed: &[u8]) -> ProtocolState {
        if self.accel_proximity != AccelerometerProximityState::Listening
            || self.accel_local_envelope.is_empty()
        {
            return self.state.clone();
        }
        let (Some(key), Some(peer_sid)) = (self.transport_key, self.peer_session_id) else {
            return self.state.clone(); // F5: no transport_key yet -> drop
        };
        if let Some(peer_envelope) = accel_envelope::open_envelope(&key, &peer_sid, sealed) {
            let correlation = crate::exchange::accelerometer::cross_correlate(
                &self.accel_local_envelope,
                &peer_envelope,
            );
            let threshold = crate::exchange::accelerometer::AccelerometerConfig::default()
                .correlation_threshold;
            let outcome = if correlation >= threshold {
                AccelerometerProximityState::Confirmed
            } else {
                AccelerometerProximityState::Failed
            };
            // Listening -> Confirmed and Listening -> Failed are both in the
            // graph, so this always succeeds; the transition must still run in
            // release (debug_assert would compile the call out), so evaluate it
            // unconditionally and only assert the must-use result in debug.
            if self.set_accel_proximity(outcome).is_err() {
                debug_assert!(false, "valid accel transition was rejected");
            }
            // F7: drop the transient envelope once correlated.
            self.accel_local_envelope.clear();
        }
        self.state.clone()
    }

    /// Cancel the session, transitioning to Failed and clearing sensitive data.
    pub fn cancel(&mut self) {
        self.state = ProtocolState::Failed("cancelled".to_string());
        self.clear_sensitive();
    }

    /// Get the QR payload to display based on current state.
    ///
    /// Returns `None` after the finalized grace period expires or after
    /// the FAIL broadcast window closes.
    pub fn get_display_qr(&mut self) -> Option<QrPayload> {
        match &self.state {
            ProtocolState::Idle => {
                let qr_data = self.build_init_qr();
                self.init_qr_cache = Some(qr_data.clone());
                self.state = ProtocolState::Advertising;
                // Use "L" error correction for INIT/INID — produces less dense QR
                // that scans faster on older cameras (Samsung S7).
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "L".to_string(),
                    display_duration_ms: jittered(DISPLAY_MS_INIT),
                })
            }
            ProtocolState::Advertising => {
                let qr_data = self
                    .init_qr_cache
                    .clone()
                    .unwrap_or_else(|| self.build_init_qr());
                Some(QrPayload {
                    data: qr_data,
                    error_correction: "L".to_string(),
                    display_duration_ms: jittered(DISPLAY_MS_INIT),
                })
            }
            ProtocolState::Discovered => {
                // Transient state — should move to Transferring quickly
                self.display_cycle += 1;
                self.get_data_chunk_qr()
            }
            ProtocolState::Transferring { .. } => {
                self.display_cycle += 1;
                // Roughly one frame in four re-shows INIT so a slower peer
                // still in Advertising can discover us (fixes the race where
                // one device transitions to Transferring before the other
                // scans our INIT). Drawn, not counted — see `random_below`.
                if random_below(4) == 0 {
                    let qr_data = self
                        .init_qr_cache
                        .clone()
                        .unwrap_or_else(|| self.build_init_qr());
                    Some(QrPayload {
                        data: qr_data,
                        error_correction: "M".to_string(),
                        // INIT dwell, not DATA: this frame exists so a peer
                        // still in Advertising can discover us, and that peer
                        // is scanning for an INIT at INIT cadence. Emitting
                        // the rescue frame at the shorter DATA dwell made the
                        // recovery path itself too brief to capture.
                        display_duration_ms: jittered(DISPLAY_MS_INIT),
                    })
                } else {
                    self.get_data_chunk_qr()
                }
            }
            ProtocolState::Verifying => {
                self.display_cycle += 1;
                // Offer VRFY *and* CONF (randomized): a peer that has already
                // reached Confirming needs our CONF (card hash), which is
                // always computable here — without it, that peer starves
                // (device-proven deadlock 2026-07-24). No shake in Verifying.
                let qr = self.build_finalization_qr(false);
                // If we stashed the peer's reveal key (received VRFY while still
                // Transferring), process it now that we've generated our own VRFY
                // for the peer to scan.
                self.try_process_stashed_reveal_key();
                Some(qr)
            }
            ProtocolState::Confirming => {
                self.display_cycle += 1;
                // TapHoverShake exchanges its accel envelope via SHAK on phase 6
                // (advisory co-location signal); preserve that. Otherwise emit
                // the dense COMBO (VRFY+CONF+RDYY) rather than cycling VRFY/CONF:
                // two peers both in Confirming each need the *other's* CONF to
                // advance, and catching one specific frame type out of a
                // multi-type cycle is decode-phase-lockable — it deadlocked both
                // sides for the full timeout (device-proven 2026-07-25
                // Pixel↔Samsung: both stuck in Confirming ~2min, both failed). A
                // single COMBO decode advances the peer Confirming→Complete→
                // Finalized via `handle_combo`, so the handshake no longer
                // depends on catching a specific type (COMBO decodability on
                // these devices is proven — the Complete arm already emits it and
                // the 07:40 run decoded it). SHAK gate mirrors
                // `build_finalization_qr`; fall back to the VRFY/CONF cycle only
                // if the COMBO can't be built.
                if self.display_cycle % 7 == 6
                    && let Some(shake_qr) = self.build_shake_qr()
                {
                    Some(QrPayload {
                        data: shake_qr,
                        error_correction: "M".to_string(),
                        display_duration_ms: jittered(DISPLAY_MS_CONF),
                    })
                } else {
                    self.get_combo_qr()
                        .or_else(|| Some(self.build_finalization_qr(true)))
                }
            }
            ProtocolState::Complete | ProtocolState::RetryReady => {
                // S1: Wall-clock timeout with progress extension.
                let now = self.monotonic.now();
                let entered = *self.phase_entered_at.get_or_insert(now);

                // Compute deadline: base + extension from last progress, capped at max.
                let progress_deadline = self
                    .last_progress_at
                    .map(|p| p + RDYY_PROGRESS_EXTENSION)
                    .unwrap_or(entered + RDYY_BASE_TIMEOUT);
                let deadline = progress_deadline
                    .max(entered + RDYY_BASE_TIMEOUT)
                    .min(entered + RDYY_MAX_TIMEOUT);

                if now > deadline {
                    // S4: Graduated retry — try once more before failing.
                    if !self.retry_used {
                        self.retry_used = true;
                        self.state = ProtocolState::RetryReady;
                        self.phase_entered_at = Some(now);
                        self.last_progress_at = None;
                        self.display_cycle = 0;
                        // Fall through to display RDYY
                    } else {
                        // Both attempts exhausted — fail with FAIL broadcast.
                        self.state =
                            ProtocolState::Failed("peer did not confirm readiness".to_string());
                        self.fail_broadcast_until = Some(now + FAIL_BROADCAST_DURATION);
                        return self.get_fail_qr();
                    }
                }

                self.display_cycle += 1;

                // Interleave DATA with COMBO: if our outbound chunks aren't
                // fully ACK'd, the peer still needs our DATA. Show DATA every
                // 3rd cycle so the peer can receive chunks while also getting
                // COMBO for the stages it's ready for.
                let all_acked = self
                    .peer_ack_bitmap
                    .as_ref()
                    .map(|b| b.is_complete())
                    .unwrap_or(false);
                // Interleave DATA (3 per 7 cycles) when peer hasn't ACK'd.
                let phase = self.display_cycle % 7;
                if !all_acked && phase < 3 {
                    self.get_data_chunk_qr().or_else(|| self.get_combo_qr())
                } else {
                    self.get_combo_qr()
                }
            }
            ProtocolState::Finalized => {
                // Continue broadcasting COMBO for a grace period so the peer
                // can scan it and also finalize.
                let now = self.monotonic.now();
                let entered = *self.phase_entered_at.get_or_insert(now);
                if now.duration_since(entered) > FINALIZED_GRACE_DURATION {
                    return None;
                }
                // Also interleave DATA if peer hasn't ACK'd all chunks.
                let all_acked = self
                    .peer_ack_bitmap
                    .as_ref()
                    .map(|b| b.is_complete())
                    .unwrap_or(false);
                let phase = self.display_cycle % 7;
                if !all_acked && phase < 3 {
                    self.display_cycle += 1;
                    self.get_data_chunk_qr().or_else(|| self.get_combo_qr())
                } else {
                    self.display_cycle += 1;
                    self.get_combo_qr()
                }
            }
            ProtocolState::Failed(_) => {
                // S5: Broadcast FAIL QR so peer aborts immediately.
                if self
                    .fail_broadcast_until
                    .is_some_and(|until| self.monotonic.now() <= until)
                {
                    return self.get_fail_qr();
                }
                None
            }
        }
    }

    /// Process a scanned QR code from the peer.
    ///
    /// Returns the new protocol state after processing.
    pub fn process_scanned_qr(&mut self, raw: &str) -> ProtocolState {
        let parsed = match qr_codec::parse_qr(raw) {
            Ok(p) => p,
            Err(_) => return self.state.clone(),
        };

        match parsed {
            StageQr::Init {
                session_id,
                ephemeral,
                commitment_hash,
                display_name: _,
                relay_url,
            } => self.handle_init(session_id, ephemeral, commitment_hash, relay_url),
            StageQr::Data {
                session_id: _,
                chunk_idx,
                chunk_total,
                ack_bitmap,
                crc: _,
                payload,
            } => self.handle_data(chunk_idx, chunk_total, ack_bitmap, payload),
            StageQr::Verify {
                session_id: _,
                reveal_key,
            } => self.handle_verify(reveal_key),
            StageQr::Confirm {
                session_id: _,
                payload_hash,
            } => self.handle_confirm(payload_hash),
            StageQr::Ready {
                session_id: _,
                ack_hash,
            } => self.handle_ready(ack_hash),
            StageQr::Combo {
                session_id: _,
                reveal_key,
                payload_hash,
                ack_hash,
            } => self.handle_combo(reveal_key, payload_hash, ack_hash),
            StageQr::Inid {
                session_id,
                ephemeral,
                commitment_hash,
                display_name: _,
                relay_url,
                ciphertext,
            } => self.handle_inid(
                session_id,
                ephemeral,
                commitment_hash,
                relay_url,
                ciphertext,
            ),
            StageQr::Fail { session_id: _ } => self.handle_fail(),
            StageQr::Shake {
                session_id: _,
                sealed_envelope,
            } => self.handle_shake(&sealed_envelope),
        }
    }

    fn build_init_qr(&self) -> String {
        // INID (INIT+Data) disabled for now — the combined QR is too dense for
        // older cameras (Samsung S7). The COMBO QR still optimizes the RDYY phase.
        // TODO: re-enable when QR scanning reliability improves (bigger screens,
        // better cameras, or binary QR mode).
        //
        // let ciphertext = self.commitment.ciphertext();
        // if ciphertext.len() <= CHUNK_PAYLOAD_SIZE {
        //     return qr_codec::format_in2d_qr(...);
        // }
        qr_codec::format_ini2_qr_with_relay(
            &self.session_id,
            self.ephemeral_public.as_bytes(),
            self.commitment.hash(),
            &self.display_name,
            self.our_relay_url.as_deref(),
        )
    }

    fn handle_init(
        &mut self,
        session_id: [u8; 16],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        relay_url: Option<String>,
    ) -> ProtocolState {
        // Only accept INIT while Advertising
        if !matches!(self.state, ProtocolState::Advertising) {
            return self.state.clone();
        }

        self.peer_session_id = Some(session_id);
        self.peer_ephemeral = Some(ephemeral);
        self.peer_commitment_hash = Some(commitment_hash);
        self.peer_relay_url = relay_url;

        match self.ephemeral_secret.take() {
            Some(secret) => {
                let peer_public = X25519Public::from(ephemeral);
                let shared_secret = secret.diffie_hellman(&peer_public);
                if !shared_secret.was_contributory() {
                    self.state = ProtocolState::Failed("non-contributory DH output".to_string());
                    return self.state.clone();
                }
                let transport_key = self.derive_transport_key(shared_secret.as_bytes());
                self.transport_key = Some(transport_key);
                // Retain the ephemeral for the post-finalize ratchet bootstrap
                // (Option A). `ephemeral_secret` stays `None` so the
                // double-key-agreement guard above still fires.
                self.ratchet_ephemeral = Some(secret);
            }
            _ => {
                self.state = ProtocolState::Failed("ephemeral secret already consumed".to_string());
                return self.state.clone();
            }
        }

        self.prepare_outbound_chunks();

        // Transition to Transferring (skip Discovered for efficiency)
        self.update_transfer_state();
        self.state.clone()
    }

    /// Handle INID (INIT+Data) — processes INIT fields and stores embedded ciphertext.
    /// Eliminates the DATA phase for small payloads (1 chunk).
    fn handle_inid(
        &mut self,
        session_id: [u8; 16],
        ephemeral: [u8; 32],
        commitment_hash: [u8; 32],
        relay_url: Option<String>,
        ciphertext: Vec<u8>,
    ) -> ProtocolState {
        let state = self.handle_init(session_id, ephemeral, commitment_hash, relay_url);

        if matches!(state, ProtocolState::Failed(_)) {
            return state;
        }

        // Store ciphertext directly (already commitment-encrypted).
        // Set up inbound tracking as if we received all DATA chunks.
        self.inbound_buffer = Some(ReassemblyBuffer::from_complete(ciphertext));
        self.inbound_bitmap = Some({
            let mut b = ChunkBitmap::new(1);
            b.mark_received(0);
            b
        });
        self.peer_chunks_total = Some(1);
        // All chunks received — should advance to Verifying
        self.update_transfer_state();
        self.state.clone()
    }

    fn handle_data(
        &mut self,
        chunk_idx: u16,
        chunk_total: u16,
        ack_bitmap_bytes: Vec<u8>,
        encrypted_payload: Vec<u8>,
    ) -> ProtocolState {
        // Accept DATA in Transferring, Discovered, or Verifying.
        // In Verifying, we still need to process the peer's ACK bitmap updates
        // so they learn which of their chunks we received (asymmetric payload sizes
        // can cause one side to reach Verifying before the other finishes transfer).
        if !matches!(
            self.state,
            ProtocolState::Transferring { .. }
                | ProtocolState::Discovered
                | ProtocolState::Verifying
        ) {
            return self.state.clone();
        }

        let transport_key = match &self.transport_key {
            Some(k) => *k,
            None => return self.state.clone(),
        };

        if self.inbound_buffer.is_none() {
            self.inbound_buffer = Some(ReassemblyBuffer::new(chunk_total));
            self.inbound_bitmap = Some(ChunkBitmap::new(chunk_total));
            self.peer_chunks_total = Some(chunk_total);
        }

        // Transport-decrypt the chunk. On AEAD failure or malformed
        // ciphertext, increment `transport_decrypt_failures` so callers can
        // distinguish "chunk hasn't arrived" from "chunk arrived corrupted
        // or tampered" (site 1 of silent-failures-in-security-paths). The
        // buffer/bitmap stay untouched so a legitimate retransmit at this
        // chunk_idx still succeeds — the counter is purely observational.
        match self.transport_decrypt(&transport_key, chunk_idx, &encrypted_payload) {
            Ok(decrypted) => {
                if let Some(ref mut buffer) = self.inbound_buffer {
                    buffer.insert(chunk_idx, decrypted);
                }
                if let Some(ref mut bitmap) = self.inbound_bitmap {
                    bitmap.mark_received(chunk_idx);
                }
            }
            Err(_) => {
                self.transport_decrypt_failures = self.transport_decrypt_failures.saturating_add(1);
                // Dev instrumentation (dev-logging only; no PII — indices only).
                // A chunk that keeps failing AEAD is never marked received, so
                // transfer stalls at "N/M" with the missing chunk invisible
                // (2026-07-25 Pixel↔Samsung transfer-stall diagnosis).
                tracing::info!(
                    "[MSX] chunk decrypt FAIL idx={chunk_idx} total={chunk_total} \
                     (corrupt/tampered — not marked received)"
                );
            }
        }

        // Update peer's ACK bitmap (tells us which of our chunks they've received)
        self.peer_ack_bitmap = Some(ChunkBitmap::from_bytes(
            &ack_bitmap_bytes,
            self.outbound_total,
        ));

        // Only advance transfer state if not already past Transferring.
        // In Verifying, we accept DATA only for the ACK bitmap update.
        if !matches!(self.state, ProtocolState::Verifying) {
            self.update_transfer_state();
        }
        self.state.clone()
    }

    fn handle_verify(&mut self, reveal_key: [u8; 32]) -> ProtocolState {
        // Accept VRFY in Verifying or Transferring.
        // With asymmetric payload sizes one side may reach Verifying while the other
        // is still Transferring. Receiving VRFY while Transferring means the peer
        // has all our chunks and has moved to Verifying; we stash the reveal key and
        // fast-track to Verifying so we can send our own VRFY in the next display cycle.
        if matches!(self.state, ProtocolState::Transferring { .. }) {
            self.peer_reveal_key = Some(reveal_key);
            // Fast-track: peer sending VRFY proves they have all our chunks.
            // Move to Verifying so we send our own VRFY to the peer.
            self.state = ProtocolState::Verifying;
            return self.state.clone();
        }

        if !matches!(self.state, ProtocolState::Verifying) {
            return self.state.clone();
        }

        // Process the reveal key (either just received, or stashed earlier)
        self.process_reveal_key(reveal_key)
    }

    /// Attempt to verify and decrypt peer data using the given reveal key.
    /// Called from handle_verify and from try_process_stashed_reveal_key.
    fn process_reveal_key(&mut self, reveal_key: [u8; 32]) -> ProtocolState {
        let ciphertext = match self.inbound_buffer.as_ref().and_then(|b| b.assemble()) {
            Some(ct) => ct,
            None => {
                self.state = ProtocolState::Failed("incomplete reassembly".to_string());
                return self.state.clone();
            }
        };

        let peer_hash = match &self.peer_commitment_hash {
            Some(h) => *h,
            None => {
                self.state = ProtocolState::Failed("missing peer commitment hash".to_string());
                return self.state.clone();
            }
        };

        // Build peer's commitment context from their relay metadata (T1.7)
        let peer_context = Self::build_commitment_context(self.peer_relay_url.as_deref());

        if !Commitment::verify_hash_with_context(
            &reveal_key,
            &ciphertext,
            &peer_hash,
            &peer_context,
        ) {
            self.state = ProtocolState::Failed("commitment verification failed".to_string());
            return self.state.clone();
        }

        match Commitment::open_with_key(&reveal_key, &ciphertext) {
            Ok(plaintext) => {
                self.received_data = Some(plaintext);
                self.peer_reveal_key = Some(reveal_key);
                self.state = ProtocolState::Confirming;
            }
            Err(_) => {
                self.state = ProtocolState::Failed("decryption failed".to_string());
            }
        }

        self.state.clone()
    }

    /// If we received the peer's reveal key while still Transferring, it was
    /// stashed in `peer_reveal_key` without decryption. Once we reach Verifying
    /// and have displayed our own VRFY, process the stashed key.
    fn try_process_stashed_reveal_key(&mut self) {
        if self.received_data.is_some() {
            return; // Already processed
        }
        if let Some(key) = self.peer_reveal_key {
            self.process_reveal_key(key);
        }
    }

    fn handle_confirm(&mut self, payload_hash: [u8; 32]) -> ProtocolState {
        if !matches!(self.state, ProtocolState::Confirming) {
            return self.state.clone();
        }

        // Verify: payload_hash should be SHA-256 of peer's original plaintext
        // The peer sends SHA-256(their_plaintext), which should match
        // SHA-256(what we decrypted from their commitment)
        let expected_hash = match &self.received_data {
            Some(data) => self.compute_card_hash(data),
            None => {
                self.state = ProtocolState::Failed("no received data to confirm".to_string());
                return self.state.clone();
            }
        };

        if bool::from(payload_hash.ct_eq(&expected_hash)) {
            self.state = ProtocolState::Complete;
            // S1: Start wall-clock timer for the RDYY phase.
            self.phase_entered_at = Some(self.monotonic.now());
            self.last_progress_at = None;
            self.display_cycle = 0;
        } else {
            self.state = ProtocolState::Failed("confirmation mismatch".to_string());
        }

        self.state.clone()
    }

    fn prepare_outbound_chunks(&mut self) {
        let ciphertext = self.commitment.ciphertext();
        let chunker = Chunker::new(ciphertext, CHUNK_PAYLOAD_SIZE);
        let total = chunker.total_chunks();
        self.outbound_total = total;

        let transport_key = match &self.transport_key {
            Some(k) => *k,
            None => return,
        };

        let mut chunks = Vec::with_capacity(total as usize);
        for i in 0..total {
            if let Some(chunk_data) = chunker.chunk(i) {
                let encrypted = self.transport_encrypt(&transport_key, i, chunk_data);
                chunks.push(encrypted);
            }
        }
        self.outbound_chunks = chunks;
        self.pending_chunk_order.clear();
    }

    fn get_data_chunk_qr(&mut self) -> Option<QrPayload> {
        if self.outbound_chunks.is_empty() {
            return None;
        }

        let total = self.outbound_total;

        // Show each chunk the peer still needs once per pass, in a fresh
        // random order. A fixed rotation ties the chunk index to the display
        // cycle and a camera sampling on its own clock then receives one index
        // forever (see `random_below`); drawing independently every frame
        // would instead cost a coupon-collector factor on a large card.
        // Reshuffling per pass keeps full coverage in `n` frames with no fixed
        // phase for a scanner to lock onto.
        let mut order = std::mem::take(&mut self.pending_chunk_order);
        order.retain(|idx| !self.peer_ack_bitmap.as_ref().is_some_and(|b| b.has(*idx)));
        if order.is_empty() {
            order = (0..total)
                .filter(|idx| !self.peer_ack_bitmap.as_ref().is_some_and(|b| b.has(*idx)))
                .collect();
            for i in (1..order.len()).rev() {
                order.swap(i, random_below(i as u32 + 1) as usize);
            }
        }

        // Everything is ACK'd: chunk 0 goes out as the carrier for our own ACK
        // bitmap, which is how the peer learns which of *their* chunks we hold.
        let idx = order.pop().unwrap_or(0);
        self.pending_chunk_order = order;

        let ack_bytes = self
            .inbound_bitmap
            .as_ref()
            .map(|b| b.to_bytes())
            .unwrap_or_default();

        let chunk_data = &self.outbound_chunks[idx as usize];
        let qr_data =
            qr_codec::format_data_qr(&self.session_id, idx, total, &ack_bytes, chunk_data);

        Some(QrPayload {
            data: qr_data,
            error_correction: "L".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_DATA),
        })
    }

    fn update_transfer_state(&mut self) {
        let all_ours_acked = self
            .peer_ack_bitmap
            .as_ref()
            .map(|b| b.is_complete())
            .unwrap_or(false);

        let all_theirs_received = self
            .inbound_buffer
            .as_ref()
            .map(|b| b.is_complete())
            .unwrap_or(false);

        let chunks_acked = self
            .peer_ack_bitmap
            .as_ref()
            .map(|b| b.received_count())
            .unwrap_or(0);
        let chunks_received = self
            .inbound_buffer
            .as_ref()
            .map(|b| b.received_count())
            .unwrap_or(0);
        let peer_total = self.peer_chunks_total.unwrap_or(0);

        // Dev instrumentation (dev-logging only; no PII — counts + booleans).
        // Distinguishes the two ways a "Transferring N/M" stall arises:
        // `ack` = our chunks the peer has confirmed, `recv` = their chunks we
        // hold. A stall with `theirs_recv=false` means we're missing an inbound
        // chunk (decode phase-lock or repeated AEAD failure); with
        // `ours_acked=false` the peer never confirmed one of ours
        // (2026-07-25 Pixel↔Samsung transfer-stall diagnosis).
        // Which indices are outstanding, not just how many. A stall pins the
        // same pair every tick, and the index is what separates "one specific
        // chunk never lands" from "progress is merely slow"
        // (2026-08-18-hover-transfer-stalls-on-the-last-chunk).
        let unacked: Vec<u16> = (0..self.outbound_total)
            .filter(|i| {
                !self
                    .peer_ack_bitmap
                    .as_ref()
                    .map(|b| b.has(*i))
                    .unwrap_or(false)
            })
            .collect();
        let missing: Vec<u16> = (0..peer_total)
            .filter(|i| {
                !self
                    .inbound_bitmap
                    .as_ref()
                    .map(|b| b.has(*i))
                    .unwrap_or(false)
            })
            .collect();
        tracing::info!(
            "[MSX] xfer ack={chunks_acked}/{} recv={chunks_received}/{peer_total} \
             ours_acked={all_ours_acked} theirs_recv={all_theirs_received} \
             unacked={unacked:?} missing={missing:?} decrypt_fail={}",
            self.outbound_total,
            self.transport_decrypt_failures
        );

        if all_ours_acked && all_theirs_received {
            self.state = ProtocolState::Verifying;
        } else {
            self.state = ProtocolState::Transferring {
                chunks_sent: chunks_acked,
                chunks_total: self.outbound_total,
                chunks_received,
                peer_chunks_total: peer_total,
            };
        }
    }

    fn derive_transport_key(&self, shared_secret: &[u8]) -> [u8; 32] {
        use crate::crypto::kdf::HKDF;
        let prk = HKDF::extract(None, shared_secret);
        let okm = HKDF::expand(&prk, HKDF_INFO, 32).expect("HKDF expand failed");
        let mut key = [0u8; 32];
        key.copy_from_slice(&okm);
        key
    }

    fn transport_encrypt(&self, key: &[u8; 32], chunk_idx: u16, plaintext: &[u8]) -> Vec<u8> {
        let nonce_bytes: [u8; 12] = crate::crypto::random_bytes();
        let idx_aad = chunk_idx.to_le_bytes();

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(&nonce_bytes);

        let payload = Payload {
            msg: plaintext,
            aad: &idx_aad,
        };
        let ciphertext = cipher.encrypt(nonce, payload).expect("encryption failed");

        // nonce || ciphertext+tag
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(&nonce_bytes);
        result.extend_from_slice(&ciphertext);
        result
    }

    /// Count of inbound DATA chunks rejected by the transport AEAD layer.
    ///
    /// Counts both malformed framing (ciphertext shorter than nonce+tag)
    /// and AEAD-verification failures (wrong key, tampered ciphertext,
    /// or replay). A nonzero count combined with stalled progress is
    /// the signal site 1 of `2026-05-21-silent-failures-in-security-paths`
    /// asked for: pre-2026-05-23 these failures were indistinguishable
    /// from "the chunk just hasn't arrived yet" because
    /// `transport_decrypt` returned `Option<Vec<u8>>` and the caller
    /// silently skipped on `None`. The counter saturates at `u32::MAX`
    /// to avoid panicking on a pathological flood.
    pub fn transport_decrypt_failure_count(&self) -> u32 {
        self.transport_decrypt_failures
    }

    fn transport_decrypt(
        &self,
        key: &[u8; 32],
        chunk_idx: u16,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>, TransportDecryptError> {
        if ciphertext.len() < 12 + 16 {
            return Err(TransportDecryptError::CiphertextTooShort);
        }
        let (nonce_bytes, encrypted) = ciphertext.split_at(12);
        let idx_aad = chunk_idx.to_le_bytes();

        let cipher = ChaCha20Poly1305::new(key.into());
        let nonce = chacha20poly1305::Nonce::from_slice(nonce_bytes);

        let payload = Payload {
            msg: encrypted,
            aad: &idx_aad,
        };
        cipher
            .decrypt(nonce, payload)
            .map_err(|_| TransportDecryptError::AeadFailure)
    }

    fn compute_card_hash(&self, data: &[u8]) -> [u8; 32] {
        let d = Sha256::digest(data);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        hash
    }

    /// Build commitment context from relay metadata (T1.7).
    ///
    /// Both sides must use the same context construction when creating and
    /// verifying the commitment hash. The context binds relay fields into
    /// the commitment so a MitM cannot swap relay URLs in the INIT QR.
    fn build_commitment_context(relay_url: Option<&str>) -> Vec<u8> {
        // Length-delimited encoding prevents ambiguity between
        // (url_A || pubkey_A) and (url_B || pubkey_B) where url_A is a
        // prefix of url_B. Without delimiters, SHA-256 pre-images could collide.
        let mut context = Vec::new();
        if let Some(url) = relay_url {
            let len = (url.len() as u32).to_be_bytes();
            context.extend_from_slice(&len);
            context.extend_from_slice(url.as_bytes());
        }
        context
    }

    fn handle_ready(&mut self, ack_hash: [u8; 32]) -> ProtocolState {
        // Accept READY in Complete or RetryReady states.
        if !matches!(
            self.state,
            ProtocolState::Complete | ProtocolState::RetryReady
        ) {
            return self.state.clone();
        }

        // S6: Any RDYY scan is progress — extend the deadline.
        self.last_progress_at = Some(self.monotonic.now());

        // Verify ack_hash matches our computation.
        let expected = self.compute_ready_hash();
        if bool::from(ack_hash.ct_eq(&expected)) {
            self.state = ProtocolState::Finalized;
            // Reset for the finalized grace period (wall-clock based).
            self.phase_entered_at = Some(self.monotonic.now());
            self.display_cycle = 0;
        }
        // Ignore mismatched READY (could be from a different exchange)

        self.state.clone()
    }

    /// Handle a FAIL QR from the peer — abort immediately.
    fn handle_fail(&mut self) -> ProtocolState {
        // Don't overwrite Finalized — if we already succeeded, ignore peer's failure.
        if matches!(self.state, ProtocolState::Finalized) {
            return self.state.clone();
        }
        // Don't overwrite existing failure.
        if matches!(self.state, ProtocolState::Failed(_)) {
            return self.state.clone();
        }
        self.state = ProtocolState::Failed("peer reported failure".to_string());
        self.state.clone()
    }

    /// Generate a FAIL QR payload for broadcasting failure to peer.
    fn get_fail_qr(&self) -> Option<QrPayload> {
        let qr_data = qr_codec::format_fail_qr(&self.session_id);
        Some(QrPayload {
            data: qr_data,
            error_correction: "L".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_FAIL),
        })
    }

    /// Generate a COMBO QR containing VRFY + CONF + RDYY.
    /// One scan by the peer can advance through all remaining stages at once.
    fn get_combo_qr(&self) -> Option<QrPayload> {
        let ack_hash = self.compute_ready_hash();
        let card_hash = self.compute_card_hash(&self.local_card);
        let qr_data = qr_codec::format_combo_qr(
            &self.session_id,
            self.commitment.reveal_key(),
            &card_hash,
            &ack_hash,
        );
        Some(QrPayload {
            data: qr_data,
            // COMBO is the densest QR (~172 chars). Use Q (25% error recovery)
            // instead of M (15%) — compensates for brightness asymmetry and
            // camera-screen distance at the scanning margin.
            error_correction: "Q".to_string(),
            display_duration_ms: jittered(DISPLAY_MS_RDYY),
        })
    }

    /// The finalization COMBO (VRFY+CONF+RDYY) for this session,
    /// independent of the display cycle and grace window. The engine seeds
    /// the `Finalized` success-screen broadcast with this so a still-`Complete`
    /// peer always scans our RDYY — never a stale DATA frame the frozen
    /// single-frame broadcast would otherwise inherit from the `Complete`-state
    /// interleave (device-proven half-exchange, 2026-07-25 Pixel↔Samsung Hover).
    pub fn finalization_combo_qr(&self) -> Option<QrPayload> {
        self.get_combo_qr()
    }

    /// Handle a COMBO QR from the peer — process VRFY + CONF + RDYY in one shot.
    /// Allows jumping from Verifying/Confirming/Complete straight to Finalized.
    ///
    /// SAFETY: Only processes VRFY if we have all inbound chunks. If we're still
    /// Transferring without complete data, the COMBO is treated as a stashed
    /// reveal key (same as receiving a standalone VRFY during Transferring).
    fn handle_combo(
        &mut self,
        reveal_key: [u8; 32],
        payload_hash: [u8; 32],
        ack_hash: [u8; 32],
    ) -> ProtocolState {
        // If still Transferring, stash the reveal key but don't chain further.
        // We can't process CONF/RDYY without the actual data.
        if matches!(self.state, ProtocolState::Transferring { .. }) {
            // handle_verify will stash the reveal key if chunks aren't complete
            self.handle_verify(reveal_key);
            // Don't chain — we need more DATA chunks first
            return self.state.clone();
        }

        // In Verifying: process reveal key → should move to Confirming
        if matches!(self.state, ProtocolState::Verifying) {
            self.handle_verify(reveal_key);
        }

        // In Confirming: process payload hash → should move to Complete
        if matches!(self.state, ProtocolState::Confirming) {
            self.handle_confirm(payload_hash);
        }

        // In Complete/RetryReady: process ack hash → should move to Finalized
        if matches!(
            self.state,
            ProtocolState::Complete | ProtocolState::RetryReady
        ) {
            self.handle_ready(ack_hash);
        }

        self.state.clone()
    }

    /// Compute the READY acknowledgment hash.
    ///
    /// SHA-256(min(our_session_id, peer_session_id) || max(our_session_id, peer_session_id))
    ///
    /// Using sorted session IDs ensures both sides compute the same hash
    /// regardless of who initiated the exchange.
    fn compute_ready_hash(&self) -> [u8; 32] {
        let peer_sid = self.peer_session_id.unwrap_or([0u8; 16]);
        let (first, second) = key_order::sorted_pair(&self.session_id, &peer_sid);

        let mut input = Vec::with_capacity(32);
        input.extend_from_slice(first);
        input.extend_from_slice(second);
        let d = Sha256::digest(&input);
        let mut hash = [0u8; 32];
        hash.copy_from_slice(d.as_ref());
        hash
    }

    fn clear_sensitive(&mut self) {
        if let Some(ref mut key) = self.transport_key {
            key.zeroize();
        }
        if let Some(ref mut key) = self.peer_reveal_key {
            key.zeroize();
        }
        self.transport_key = None;
        self.peer_reveal_key = None;
        self.ephemeral_secret = None;
        self.ratchet_ephemeral = None;
        self.outbound_chunks.clear();
        self.inbound_buffer = None;
        self.received_data = None;
        self.accel_local_envelope.clear();
    }
}

impl Drop for MultiStageSession {
    fn drop(&mut self) {
        self.clear_sensitive();
    }
}
