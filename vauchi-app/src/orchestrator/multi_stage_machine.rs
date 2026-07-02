// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage face-to-face exchange state machine (slice 32m Phase 1).
//!
//! Replaces the spawned `vauchi-exchange-cycle` thread in
//! `core/vauchi-platform/src/multistage_exchange.rs` with a
//! deterministic, synchronous machine the engine owns and advances via
//! the `poll_notifications` tick — **one non-blocking step per
//! [`MultiStageMachine::advance`]**.
//!
//! Pattern source: slice 32l's
//! [`DeviceLinkInitiatorMachine`](super::device_link_machine::DeviceLinkInitiatorMachine).
//! Same five-method shape (`new` / `phase` / `advance` / `handle_hardware_event`
//! / `cancel`), same `now: u64` time discipline (no `Sleeper`,
//! no `Clock`, no thread, no mpsc — CC-06).
//!
//! Design:
//! `_private/docs/designs/2026-05-28-slice-32m-multi-stage-event-command-mapping-design.md`.
//!
//! Phase sequence (extracted from
//! `vauchi-platform/src/multistage_exchange.rs::cycle_loop` and
//! `vauchi-core/src/exchange/multistage/session.rs::get_display_qr`):
//!
//! ```text
//! new ─(no I/O)→ Preparing
//! Preparing ─advance: emit first INIT QR (Idle→Advertising)─▶ Advertising
//! Advertising ─advance per display_duration_ms tick─▶ Advertising (next QR)
//! Advertising ─QrScanned: peer INIT parsed─▶ Discovered
//! Discovered ─advance/QrScanned: chunk transfer─▶ Transferring{…}
//! Transferring ─advance/QrScanned: protocol verify─▶ Verifying
//! Verifying ─advance/QrScanned: peer ack─▶ Confirming
//! Confirming ─advance/QrScanned: finalize─▶ Finalized {peer_name}
//! Finalized ─advance: cycle-end persistence done─▶ Completed
//! (any) ─HardwareError / PermissionDenied / fatal Unavailable─▶ Failed{reason}
//! (any) ─cancel─▶ Cancelled (absorbing)
//! ```
//!
//! **No relay I/O.** The multi-stage flow is in-person (BLE-less);
//! `advance` only drives the inner [`MultiStageSession`] state machine
//! and emits side-effect commands (QR display, screen-presentation
//! hardware) via the returned [`MultiStageEvent`]. Tests drive expiry
//! and per-frame timing by passing `now` values — no real wait
//! (CC-06).
//!
//! # T1.2 status (GREEN — minimum viable)
//!
//! `advance` + the `QrScanned` branch of `handle_hardware_event` are
//! wired against [`MultiStageSession::get_display_qr`] /
//! [`MultiStageSession::process_scanned_qr`]. The T1.1 proptest
//! invariants pass — the per-frame display-duration window is honoured
//! via `now: u64`, `Finalized → Completed` is reachable through the
//! existing `ProtocolState` graph, and the terminal-absorption /
//! no-QR-after-terminal invariants are preserved.
//!
//! AppEngine integration (`sync_/ensure_/advance_/dispatch_/apply_`
//! five-method on `ui/app_engine/multi_stage_exchange.rs`) and the
//! Hover audio-handshake command emission are T1.2b follow-ups. Until
//! they land, the existing cycle-thread bridge in
//! `vauchi-platform/src/multistage_exchange.rs` continues to drive the
//! engine — the new machine coexists with the old surface, which T1.3
//! retires when the bridge is no longer the only path.

use vauchi_core::Command;
use vauchi_core::Event;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{
    AccelerometerProximityState, AudioConfig, AudioProximityState, MultiStageSession,
    ProtocolState, QrPayload, audio_modem,
};

/// Audio-listen window default (ms). Mirrors the cycle-thread's
/// `MobileMultiStageSession::AUDIO_LISTEN_TIMEOUT_MS`. Kept private
/// to this module — the platform binding never reads it directly.
const AUDIO_LISTEN_TIMEOUT_MS: u64 = 5000;

/// Exchange-payload format version byte. Mirrors the constant in
/// `vauchi-platform/src/mobile_exchange.rs` (which T3.1 retires) and
/// the inverse in `app_engine::multi_stage_exchange::serialize_exchange_payload`.
/// Format: `[version: 1][public_key: 32][card_json: rest]`.
const EXCHANGE_PAYLOAD_VERSION: u8 = 1;

/// Maximum a single non-terminal, peer-engaged exchange phase may persist
/// with no forward progress before the machine fails to a retry/cancel
/// screen. The deadline resets on every phase transition, so a healthy
/// exchange (steady QR/chunk progress) never trips it; only a wait state
/// with no progress does — the device-verified infinite "Searching…"
/// (problem `2026-06-11-exchange-waits-forever-without-capabilities`,
/// ADR-021: core owns the timer, never the frontend). Milliseconds — the
/// machine's time domain is `unix_millis` (per the 2026-06-03 per-frame
/// gating fix; seconds froze the QR ~1000× its window).
pub const MULTI_STAGE_STEP_TIMEOUT_MS: u64 = 120_000;

/// Deadline for the peerless `Advertising` phase. Discovery is
/// human-paced — two people opening the mode on both devices and
/// aligning cameras burned the flat 120 s step budget on-device before
/// any peer contact (Phase 1 field feedback 2026-07-02 in the record
/// above). Still finite: every waiting state keeps a core-owned deadline
/// that lands on a retry/cancel screen; a longer flat budget was chosen
/// over reset-on-scan-activity because a foreign scanner could otherwise
/// keep the session alive indefinitely.
pub const MULTI_STAGE_DISCOVERY_TIMEOUT_MS: u64 = 300_000;

/// Observable phase of the multi-stage machine. 1:1 with
/// [`ProtocolState`] (renamed for engine-side ergonomics — the
/// underlying protocol model is unchanged).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MultiStagePhase {
    /// Constructed, pre-advertise. No QR emitted yet, no `Command`
    /// for screen presentation issued. Matches the cycle-thread's
    /// pre-`start()` window.
    Preparing,
    /// Emitting our QR frames; peer not yet observed.
    Advertising,
    /// Peer QR observed, transferring not started.
    Discovered,
    /// Chunk-level transfer in progress.
    Transferring {
        chunks_sent: u16,
        chunks_total: u16,
        chunks_received: u16,
        peer_chunks_total: u16,
    },
    /// Protocol verifying chunks against the peer commitment.
    Verifying,
    /// Awaiting peer's finalize ack.
    Confirming,
    /// Peer name resolved, contact persisted; the success terminal
    /// hasn't been entered yet.
    Finalized { peer_name: String },
    /// Success terminal. Frontend may navigate away.
    Completed,
    /// Terminal failure with a stable reason id (see
    /// `Event::HardwareError`/`PermissionDenied` / protocol
    /// `Failed { reason }`).
    Failed { reason: String },
    /// User-initiated cancel (absorbing).
    Cancelled,
}

/// What a transition produced. The engine maps each to a
/// `ScreenModel` update + `Command` emission via the existing
/// `MultiStageExchangeEngine::set_*` setters.
///
/// Not `PartialEq` — [`QrPayload`] from `vauchi_core` does not derive
/// `PartialEq` and adding a manual impl just to compare in tests
/// would be a transitive obligation we don't want to take on. Tests
/// match on the variant via `matches!` instead.
#[derive(Debug, Clone)]
pub enum MultiStageEvent {
    /// No observable change this step.
    None,
    /// Our next QR frame is ready; payload + display duration the
    /// protocol just emitted.
    QrFrameReady(QrPayload),
    /// Peer was discovered; their QR parsed successfully.
    PeerDiscovered,
    /// Chunk-transfer progress changed.
    TransferProgress {
        chunks_sent: u16,
        chunks_total: u16,
        chunks_received: u16,
        peer_chunks_total: u16,
    },
    /// Verification phase entered.
    Verifying,
    /// Confirmation phase entered.
    Confirming,
    /// Protocol reached `Finalized` and the peer's display name is
    /// known (read out of the just-received card).
    Finalized { peer_name: String },
    /// Cycle thread's `on_session_ended` analogue: persistence is
    /// done and the engine may render the success terminal.
    Completed,
    /// Terminal failure.
    Failed { reason: String },
    /// Hover audio-proximity state changed (`Listening` →
    /// `Confirmed` / `Failed`) as a result of an
    /// `Event::AudioSamplesRecorded` ingress. The engine
    /// integration maps this onto `set_audio_proximity` on the
    /// active `MultiStageExchangeEngine`.
    AudioProximityChanged(AudioProximityState),
    /// TapHoverShake accelerometer-proximity state changed. The engine
    /// integration maps this onto `set_accel_proximity` on the active
    /// `MultiStageExchangeEngine` (same shape as `AudioProximityChanged`).
    AccelProximityChanged(AccelerometerProximityState),
}

/// Mode marker — Glance (bilateral QR only) vs Hover (QR + audio
/// proximity handshake). Matches the existing two-constructor
/// pattern on [`crate::ui::multi_stage_exchange::MultiStageExchangeEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiStageMode {
    Glance,
    Hover,
    /// QR + ultrasonic audio proximity (like Hover) **plus** the
    /// accelerometer shake-correlation signal — the most signal-rich
    /// face-to-face mode. Runs both proximity signals in parallel; both
    /// are advisory (neither gates completion). The audio gates below
    /// admit `TapHoverShake` alongside `Hover`; the accel signal is
    /// driven through `accel_proximity` on the inner session.
    TapHoverShake,
}

/// Deterministic, poll-driven multi-stage exchange machine.
///
/// Owns the inner [`MultiStageSession`] directly. Replaces the
/// `vauchi-platform::MobileMultiStageSession` + cycle-thread duo
/// without changing the underlying protocol behaviour — just the
/// driving discipline (one `advance` per tick, no spawned thread,
/// no listener trait).
pub struct MultiStageMachine {
    inner: MultiStageSession,
    mode: MultiStageMode,
    /// Current observable phase. Tracked alongside the inner
    /// `ProtocolState` so terminal/Cancelled override the
    /// underlying state cleanly.
    phase: MultiStagePhase,
    /// Per-frame timing: when the current QR frame was first
    /// emitted (`now` units). `None` before the first emission, set
    /// on every `get_display_qr` call inside `advance` so the next
    /// tick can decide whether `display_duration_ms` has elapsed.
    current_frame_started_at: Option<u64>,
    /// Per-frame display duration last emitted, in the same `now`
    /// units. `0` until the first frame is emitted. Paired with
    /// `current_frame_started_at` for the per-frame tick check.
    current_frame_duration: u64,
    /// `true` once `cancel` has been called. Subsequent `advance` /
    /// `handle_hardware_event` calls return [`MultiStageEvent::None`]
    /// and leave the phase at [`MultiStagePhase::Cancelled`].
    cancelled: bool,
    /// `now` (ms) when the current phase was entered. Set on
    /// construction, re-stamped on every phase transition; the per-step
    /// stall deadline ([`MULTI_STAGE_STEP_TIMEOUT_MS`]) is measured from
    /// it so steady progress refreshes the budget and only a stalled
    /// wait state trips it.
    phase_entered_ms: u64,
}

impl MultiStageMachine {
    /// Construct a Glance-mode machine — bilateral QR scan, no
    /// ultrasonic proximity handshake. **No I/O.** The first
    /// [`advance`](Self::advance) emits the first QR frame.
    pub fn new_glance(local_card: Vec<u8>, now: u64) -> Self {
        Self {
            inner: MultiStageSession::new(local_card),
            mode: MultiStageMode::Glance,
            phase: MultiStagePhase::Preparing,
            current_frame_started_at: None,
            current_frame_duration: 0,
            cancelled: false,
            phase_entered_ms: now,
        }
    }

    /// Construct a Hover-mode machine — QR + ultrasonic proximity
    /// handshake. Same I/O discipline as `new_glance`. Audio
    /// commands fire on the `Confirming` transition (per T0.2
    /// design); the listening window restarts on retry. T1.2b
    /// wires the audio command emission via `event_to_commands`.
    pub fn new_hover(local_card: Vec<u8>, now: u64) -> Self {
        Self {
            inner: MultiStageSession::new(local_card),
            mode: MultiStageMode::Hover,
            phase: MultiStagePhase::Preparing,
            current_frame_started_at: None,
            current_frame_duration: 0,
            cancelled: false,
            phase_entered_ms: now,
        }
    }

    /// Construct a TapHoverShake-mode machine — QR + ultrasonic audio
    /// proximity (like Hover) plus the accelerometer shake signal. Same
    /// I/O discipline as `new_glance`. The audio handshake fires via the
    /// shared `try_audio_handshake_start` (now un-gated for this mode);
    /// the accel capture + envelope cross-correlation is a follow-up
    /// (needs the envelope-over-transport protocol — see the TapHoverShake
    /// graduation plan P2.C / the accel-envelope ADR).
    pub fn new_tap_hover_shake(local_card: Vec<u8>, now: u64) -> Self {
        Self {
            inner: MultiStageSession::new(local_card),
            mode: MultiStageMode::TapHoverShake,
            phase: MultiStagePhase::Preparing,
            current_frame_started_at: None,
            current_frame_duration: 0,
            cancelled: false,
            phase_entered_ms: now,
        }
    }

    /// Current observable phase.
    pub fn phase(&self) -> MultiStagePhase {
        self.phase.clone()
    }

    /// Whether this machine is in a terminal phase. `true` for
    /// `Completed`, `Failed`, `Cancelled`; `false` for every other
    /// variant. Terminal phases are absorbing — subsequent
    /// `advance` / `handle_hardware_event` calls always return
    /// `MultiStageEvent::None`.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.phase,
            MultiStagePhase::Completed
                | MultiStagePhase::Failed { .. }
                | MultiStagePhase::Cancelled
        )
    }

    /// One non-blocking step. Drives the inner
    /// [`MultiStageSession`] forward by one display-frame tick:
    ///
    /// - From `Preparing`: the first call emits the INIT QR
    ///   (`get_display_qr` transitions the inner state
    ///   `Idle → Advertising`).
    /// - From `Advertising` or any later non-terminal phase: emits
    ///   the next QR frame **only** if
    ///   `now - current_frame_started_at >= current_frame_duration`,
    ///   otherwise returns `None` to honour the per-frame
    ///   `display_duration_ms` window the protocol asks for.
    /// - From a terminal phase: returns `None`.
    ///
    /// Phase is re-derived from `inner.get_state()` on every
    /// successful emission so a `Finalized` or `Failed` transition
    /// is observed in the same tick. The terminal mapping is
    /// authoritative — if the inner state machine moves to `Failed`
    /// the machine's phase flips before the next `advance` /
    /// `handle_hardware_event` call.
    pub fn advance(&mut self, now: u64) -> MultiStageEvent {
        if self.cancelled || self.is_terminal() {
            return MultiStageEvent::None;
        }
        if self.step_timed_out(now) {
            return self.fail_step_timed_out();
        }
        let prior_phase = self.phase.clone();
        let event = self.advance_frame(now);
        self.note_phase_progress(&prior_phase, now);
        event
    }

    /// Whether the current phase has stalled past its budget —
    /// [`MULTI_STAGE_DISCOVERY_TIMEOUT_MS`] for the human-paced peerless
    /// `Advertising` phase, [`MULTI_STAGE_STEP_TIMEOUT_MS`] for every
    /// peer-engaged phase. `Finalized` is success-pending (no peer wait —
    /// it never times out); `Completed`/`Failed`/`Cancelled` are excluded
    /// by the caller's `is_terminal` guard.
    fn step_timed_out(&self, now: u64) -> bool {
        let budget = match self.phase {
            MultiStagePhase::Finalized { .. } => return false,
            MultiStagePhase::Advertising => MULTI_STAGE_DISCOVERY_TIMEOUT_MS,
            _ => MULTI_STAGE_STEP_TIMEOUT_MS,
        };
        now.saturating_sub(self.phase_entered_ms) >= budget
    }

    /// Transition to the stall-timeout terminal failure with a
    /// plain-language reason. The failed screen renders it directly as the
    /// `StatusIndicator` detail, so it must be user-readable, not a stable
    /// id — matching the Direct/BLE/NFC engines' timeout messages (T1.5).
    fn fail_step_timed_out(&mut self) -> MultiStageEvent {
        let reason = "Exchange timed out — no response from the other device.".to_string();
        self.phase = MultiStagePhase::Failed {
            reason: reason.clone(),
        };
        MultiStageEvent::Failed { reason }
    }

    /// Re-stamp the per-step deadline whenever the phase advanced, so a
    /// healthy exchange (steady progress) keeps refreshing its budget
    /// and only a stalled wait state trips it.
    fn note_phase_progress(&mut self, prior: &MultiStagePhase, now: u64) {
        if self.phase != *prior {
            self.phase_entered_ms = now;
        }
    }

    /// One display-frame step. No deadline/progress bookkeeping — the
    /// public [`advance`](Self::advance) wraps this with the per-step
    /// stall deadline and phase-progress stamping.
    fn advance_frame(&mut self, now: u64) -> MultiStageEvent {
        // Per-frame gating: if the previous frame's window has not
        // elapsed yet, hold. The first frame (Preparing entry) has
        // no prior window so it emits immediately.
        if let Some(started_at) = self.current_frame_started_at
            && now.saturating_sub(started_at) < self.current_frame_duration
        {
            return MultiStageEvent::None;
        }
        let payload = match self.inner.get_display_qr() {
            Some(p) => p,
            // `None` from the inner machine means "nothing to
            // display right now" — Finalized grace window expired
            // or FAIL broadcast window closed. Re-derive the phase
            // from the inner state without emitting a frame.
            None => {
                let prior = self.phase.clone();
                self.sync_phase_from_inner_state();
                // Cycle-end: once the session is Finalized and has
                // stopped emitting QR (grace window closed), surface
                // `Completed` so the engine flips to the success screen
                // (`session_ended`). `phase_from_protocol_state` never
                // yields `Completed` — the retired cycle thread's
                // `on_session_ended` set it; the poll path dropped it,
                // leaving the machine stuck in `Finalized` forever and
                // the screen on "Almost done"
                // (2026-06-03-multistage-qr-exchange-stalls-init-on-device).
                if matches!(self.phase, MultiStagePhase::Finalized { .. }) {
                    self.phase = MultiStagePhase::Completed;
                    return phase_transition_event(&prior, &self.phase);
                }
                return MultiStageEvent::None;
            }
        };
        self.current_frame_started_at = Some(now);
        // `display_duration_ms` is `u32` on the protocol type; widen
        // to `u64` to match the machine's time domain.
        self.current_frame_duration = u64::from(payload.display_duration_ms);
        self.sync_phase_from_inner_state();
        // If the inner transition flipped us into a terminal phase
        // (e.g. the FAIL frame on a hardware-failed exchange) the
        // I4 contract is "no QrDisplay after terminal" — drop the
        // frame we just produced and return `None`.
        if self.is_terminal() {
            return MultiStageEvent::None;
        }
        MultiStageEvent::QrFrameReady(payload)
    }

    /// Translate a frontend-emitted [`vauchi_core::Event`] into a
    /// machine transition.
    ///
    /// - `QrScanned` feeds the bytes into
    ///   [`MultiStageSession::process_scanned_qr`] and re-derives the
    ///   phase from the resulting inner state.
    /// - `HardwareError`, `PermissionDenied`, and the fatal subset
    ///   of `HardwareUnavailable` (`camera`, `microphone`) flip the
    ///   machine to `Failed` with a stable reason id.
    /// - Non-fatal `HardwareUnavailable` variants
    ///   (`screen_brightness`, `idle_timer`, `orientation_lock`) are
    ///   tolerated per their `Command` docstring contracts.
    /// - `QrScanProgress` and `AudioSamplesRecorded` are inert at
    ///   this layer (viewfinder telemetry / audio handshake live
    ///   downstream of the machine — see the engine integration in
    ///   T1.2b).
    /// - Every other variant is explicitly ignored.
    pub fn handle_hardware_event(&mut self, event: &Event, now: u64) -> MultiStageEvent {
        if self.cancelled || self.is_terminal() {
            return MultiStageEvent::None;
        }
        let prior_phase = self.phase.clone();
        let result = match event {
            Event::HardwareError { transport, error } => {
                let reason = format!("{transport}: {error}");
                self.phase = MultiStagePhase::Failed {
                    reason: reason.clone(),
                };
                MultiStageEvent::Failed { reason }
            }
            Event::PermissionDenied { transport } => {
                let reason = format!("permission_denied:{transport}");
                self.phase = MultiStagePhase::Failed {
                    reason: reason.clone(),
                };
                MultiStageEvent::Failed { reason }
            }
            Event::HardwareUnavailable { transport } => match transport.as_str() {
                "camera" | "microphone" => {
                    let reason = format!("hardware_unavailable:{transport}");
                    self.phase = MultiStagePhase::Failed {
                        reason: reason.clone(),
                    };
                    MultiStageEvent::Failed { reason }
                }
                _ => MultiStageEvent::None,
            },
            Event::QrScanned { data } => {
                // Feed the scanned QR into the protocol's
                // deserializer. The inner state machine may
                // transition (Advertising → Discovered, Discovered →
                // Transferring, etc.). Malformed scans are no-ops
                // inside `process_scanned_qr`; either way we
                // re-derive our phase from the new inner state.
                let accel_before = self.inner.accel_proximity();
                let _ = self.inner.process_scanned_qr(data);
                let qr_prior_phase = self.phase.clone();
                self.sync_phase_from_inner_state();
                // A scanned SHAK stage may have driven accel-proximity
                // (open + cross-correlate) without any phase change; surface it
                // so the engine updates the advisory shake chrome. One QR is one
                // stage, so this never coincides with a phase transition.
                let accel_after = self.inner.accel_proximity();
                if accel_after != accel_before {
                    // Re-stamp the stall deadline on the outer pre-match phase
                    // before this early return, so it does not depend on the
                    // "never coincides with a transition" invariant above.
                    self.note_phase_progress(&prior_phase, now);
                    return MultiStageEvent::AccelProximityChanged(accel_after);
                }
                phase_transition_event(&qr_prior_phase, &self.phase)
            }
            Event::QrScanProgress { .. } => {
                // Per-frame viewfinder telemetry — the engine-side
                // `MultiStageExchangeEngine` consumes this for the
                // ScanQuality indicator. No protocol effect.
                MultiStageEvent::None
            }
            Event::AudioSamplesRecorded {
                samples,
                sample_rate,
            } => self.process_audio_samples(samples, *sample_rate),
            Event::AccelerometerData {
                x_milli_g,
                y_milli_g,
                z_milli_g,
                ..
            } => self.process_accel_data(*x_milli_g, *y_milli_g, *z_milli_g),
            // Every other Event variant is inert for multi-stage —
            // late BLE notifications, NFC taps, link callbacks etc.
            // arriving on the multi-stage screen are ignored to
            // keep the machine's surface narrow.
            _ => MultiStageEvent::None,
        };
        self.note_phase_progress(&prior_phase, now);
        result
    }

    /// User-initiated cancel. Idempotent — calling on an already-
    /// cancelled machine is a no-op that still returns `None`.
    pub fn cancel(&mut self) -> MultiStageEvent {
        if self.cancelled {
            return MultiStageEvent::None;
        }
        self.cancelled = true;
        self.phase = MultiStagePhase::Cancelled;
        // Wipe sensitive protocol state, mirroring the cycle thread's
        // `MultiStageSession::cancel` discipline.
        self.inner.cancel();
        MultiStageEvent::None
    }

    /// Mode marker — read-only.
    pub fn mode(&self) -> MultiStageMode {
        self.mode
    }

    /// Received exchange payload (`[version][peer_pk:32][card_json]`),
    /// available only once the inner session is `Finalized` (atomic —
    /// both sides confirmed). The AppEngine deserializes this to persist
    /// the exchanged contact — the job the retired cycle thread's
    /// `on_finalized` listener used to do
    /// (2026-06-03-multistage-qr-exchange-stalls-init-on-device).
    pub fn received_exchange_payload(&self) -> Option<Vec<u8>> {
        self.inner.get_received_data()
    }

    /// Transport key (shared-secret seed) for the exchanged contact's
    /// secure channel. `Some` once the transport DH has run.
    pub fn transport_key(&self) -> Option<[u8; 32]> {
        self.inner.get_transport_key()
    }

    /// Build the role-correct Double Ratchet for the finalized exchange.
    /// `None` if the transport key / ephemerals aren't available (the
    /// AppEngine then persists the contact without the update channel
    /// rather than dropping it entirely).
    pub fn build_exchange_ratchet(
        &self,
        our_identity: &[u8; 32],
        their_identity: &[u8; 32],
    ) -> Option<(vauchi_core::crypto::DoubleRatchetState, bool)> {
        self.inner
            .build_exchange_ratchet(our_identity, their_identity)
            .ok()
    }

    /// Hover-only side effect: fire the ultrasonic FSK
    /// handshake when the protocol observes `Discovered`.
    /// Mirrors the cycle-thread's `try_autonomous_audio_trigger`
    /// (`vauchi-platform/src/multistage_exchange.rs::try_autonomous_audio_trigger`)
    /// at the machine layer:
    ///
    /// - Mode gate: Glance returns `[]`; only Hover proceeds.
    /// - State gate: returns `[]` unless `audio_proximity` is
    ///   `Pending` (idempotent under repeated calls).
    /// - Transition: drives the inner session
    ///   `Pending -> Listening` via `set_audio_proximity`.
    ///   On rejection (already started, terminal, etc.) returns `[]`.
    /// - Emits the paired commands
    ///   `Command::AudioEmitChallenge` (FSK-encoded `session_id` as
    ///   the challenge — same temporary derivation the cycle
    ///   thread used, Phase 1.C.3e-iii) +
    ///   `Command::AudioListenForResponse` so the platform
    ///   simultaneously plays the chirp and listens for the peer's
    ///   reply within the listen-window budget.
    pub fn try_audio_handshake_start(&mut self) -> Vec<Command> {
        if !matches!(
            self.mode,
            MultiStageMode::Hover | MultiStageMode::TapHoverShake
        ) {
            return Vec::new();
        }
        if self.inner.audio_proximity() != AudioProximityState::Pending {
            return Vec::new();
        }
        let challenge = self.inner.session_id();
        let transitioned = self
            .inner
            .set_audio_proximity(AudioProximityState::Listening)
            .is_ok();
        if !transitioned {
            return Vec::new();
        }
        let config = AudioConfig::default();
        let samples = audio_modem::generate_fsk_samples(&challenge, &config);
        let sample_rate = config.sample_rate;
        vec![
            Command::AudioEmitChallenge {
                samples,
                sample_rate,
            },
            Command::AudioListenForResponse {
                timeout_ms: AUDIO_LISTEN_TIMEOUT_MS,
                sample_rate,
            },
        ]
    }

    /// TapHoverShake-only: start the accelerometer capture for the shake
    /// co-location signal. Mirrors [`Self::try_audio_handshake_start`] —
    /// mode gate (only TapHoverShake), state gate (only from `Pending`,
    /// idempotent), drives the inner session `Pending -> Listening`, and
    /// emits `Command::AccelerometerStart` so the platform streams
    /// `Event::AccelerometerData`.
    ///
    /// **Dormant in this scaffolding slice:** no autonomous caller fires it
    /// yet — the live trigger + the peer-envelope cross-correlate land with
    /// the accel-envelope-transport follow-up (ADR-009 amendment). Wired +
    /// tested here so that work is purely additive.
    pub fn try_accel_capture_start(&mut self) -> Vec<Command> {
        if self.mode != MultiStageMode::TapHoverShake {
            return Vec::new();
        }
        if self.inner.accel_proximity() != AccelerometerProximityState::Pending {
            return Vec::new();
        }
        if self
            .inner
            .set_accel_proximity(AccelerometerProximityState::Listening)
            .is_err()
        {
            return Vec::new();
        }
        vec![Command::AccelerometerStart]
    }

    /// Decode FSK samples from a frontend `AudioSamplesRecorded`
    /// event, verify against the peer's session_id, and transition
    /// the audio-proximity state. Mirrors the cycle-thread's
    /// `process_audio_samples_recorded`
    /// (`vauchi-platform/src/multistage_exchange.rs`) at the
    /// machine layer:
    ///
    /// - Decode succeeds AND bytes match peer_session_id ->
    ///   Listening -> Confirmed.
    /// - Decode succeeds but mismatch -> Listening -> Failed.
    /// - Decode fails (malformed samples, preamble not found) ->
    ///   Listening -> Failed.
    /// - peer_session_id is None (Stage 1 not yet complete) ->
    ///   Listening -> Failed.
    ///
    /// Glance flows return `MultiStageEvent::None` immediately
    /// (mode gate). Returning `None` from a non-`Listening` state
    /// is also a no-op so a stray late sample after Confirmed /
    /// Failed cannot bounce the state.
    fn process_audio_samples(&mut self, samples: &[f32], sample_rate: u32) -> MultiStageEvent {
        if !matches!(
            self.mode,
            MultiStageMode::Hover | MultiStageMode::TapHoverShake
        ) {
            return MultiStageEvent::None;
        }
        if self.inner.audio_proximity() != AudioProximityState::Listening {
            return MultiStageEvent::None;
        }
        let config = AudioConfig::default();
        let decoded = audio_modem::decode_fsk_samples(samples, sample_rate, &config);
        let next = match decoded {
            Ok(bytes) => match self.inner.verify_audio_response(&bytes) {
                Some(true) => AudioProximityState::Confirmed,
                _ => AudioProximityState::Failed,
            },
            Err(_) => AudioProximityState::Failed,
        };
        if self.inner.set_audio_proximity(next).is_err() {
            return MultiStageEvent::None;
        }
        MultiStageEvent::AudioProximityChanged(next)
    }

    /// Accumulate one `Event::AccelerometerData` sample into the local
    /// magnitude envelope (TapHoverShake only, while `accel_proximity ==
    /// Listening`). Samples are milli-g per axis; we store the Euclidean
    /// magnitude in g, matching `exchange::accelerometer`'s envelope shape.
    ///
    /// **Local-only in this slice:** the `Listening -> Confirmed/Failed`
    /// cross-correlation needs the *peer's* envelope, which arrives over the
    /// transport in the accel-envelope-transport follow-up. Until then this
    /// only builds the local envelope and returns `None`.
    fn process_accel_data(
        &mut self,
        x_milli_g: i32,
        y_milli_g: i32,
        z_milli_g: i32,
    ) -> MultiStageEvent {
        if self.mode != MultiStageMode::TapHoverShake {
            return MultiStageEvent::None;
        }
        if self.inner.accel_proximity() != AccelerometerProximityState::Listening {
            return MultiStageEvent::None;
        }
        let x = x_milli_g as f32 / 1000.0;
        let y = y_milli_g as f32 / 1000.0;
        let z = z_milli_g as f32 / 1000.0;
        // Feed the session envelope — the protocol source of truth: it seals
        // this into the SHAK QR (`get_display_qr`) and cross-correlates the
        // peer's on receive. The session re-applies the same `Listening` gate.
        self.inner
            .record_accel_envelope_samples(&[(x * x + y * y + z * z).sqrt()]);
        MultiStageEvent::None
    }

    /// Number of accelerometer magnitude samples captured into the local
    /// envelope. Public test seam (like `protocol_state_for_test`) for the
    /// dormant capture path; also the length the peer-envelope exchange reads
    /// before `encode_envelope` when the transport slice lands.
    pub fn accel_sample_count(&self) -> usize {
        self.inner.accel_envelope_len()
    }

    /// Re-derive `self.phase` from the inner [`MultiStageSession`]
    /// state. Called after every protocol-driving operation
    /// (`get_display_qr`, `process_scanned_qr`).
    fn sync_phase_from_inner_state(&mut self) {
        // Once a user cancel has been observed the phase is
        // absorbing — never let an inner transition un-cancel us.
        if self.cancelled {
            return;
        }
        let mut new_phase = phase_from_protocol_state(&self.inner.get_state());
        // Late-bind the peer display name on the Finalized
        // transition. `phase_from_protocol_state` returns an empty
        // name because the mapping is a pure function of the
        // protocol state; the name lives in the just-received
        // payload, which only the session has.
        if matches!(new_phase, MultiStagePhase::Finalized { .. }) {
            let peer_name = extract_peer_name(&self.inner);
            new_phase = MultiStagePhase::Finalized { peer_name };
        }
        // Preserve `Failed { reason }` already set by a hardware
        // failure path — the inner state machine doesn't know about
        // those reason strings; only the host-side branches do.
        if matches!(self.phase, MultiStagePhase::Failed { .. })
            && !matches!(new_phase, MultiStagePhase::Failed { .. })
        {
            return;
        }
        self.phase = new_phase;
    }
}

/// Decode the peer's display name from the just-received
/// exchange payload. Called only on the `Finalized` transition;
/// returns an empty string when the payload is absent (race —
/// Finalized observed before reassembly completes) or malformed
/// (deserialize failure — surfaces as the empty success-chrome
/// name, never panics).
///
/// Wire format mirrors `serialize_exchange_payload`
/// (`vauchi-app/src/ui/app_engine/multi_stage_exchange.rs`):
/// `[version: 1][public_key: 32][card_json: rest]`. Drops the
/// public key after the version check — the contact's signing
/// key lives in storage via the persistence path, not on the
/// success screen.
fn extract_peer_name(session: &MultiStageSession) -> String {
    let Some(data) = session.get_received_data() else {
        return String::new();
    };
    decode_peer_name_from_payload(&data)
}

/// Pure-byte counterpart of [`extract_peer_name`] split out so the
/// payload-shape edges (short input, wrong version, malformed
/// `card_json`) can be unit-tested without spinning up a full
/// `MultiStageSession` peer exchange.
fn decode_peer_name_from_payload(data: &[u8]) -> String {
    if data.len() < 34 || data[0] != EXCHANGE_PAYLOAD_VERSION {
        return String::new();
    }
    match serde_json::from_slice::<ContactCard>(&data[33..]) {
        Ok(card) => card.display_name().to_string(),
        Err(_) => String::new(),
    }
}

/// Map a [`ProtocolState`] onto a [`MultiStagePhase`].
///
/// `Complete` and `RetryReady` both fold into `Confirming` — they're
/// the protocol's "exchange-data complete, still finalizing"
/// intermediate states that the machine surfaces as the same
/// "waiting for peer ack" chrome. The Finalized peer-name is read
/// from the just-received contact card if available; absent (race
/// between Finalized and the chunk reassembly) it falls back to an
/// empty string and the engine's display string defaults to the
/// generic "Exchange Complete" wording — T1.2b's engine integration
/// re-resolves the name on the next phase observation.
fn phase_from_protocol_state(state: &ProtocolState) -> MultiStagePhase {
    match state {
        ProtocolState::Idle => MultiStagePhase::Preparing,
        ProtocolState::Advertising => MultiStagePhase::Advertising,
        ProtocolState::Discovered => MultiStagePhase::Discovered,
        ProtocolState::Transferring {
            chunks_sent,
            chunks_total,
            chunks_received,
            peer_chunks_total,
        } => MultiStagePhase::Transferring {
            chunks_sent: *chunks_sent,
            chunks_total: *chunks_total,
            chunks_received: *chunks_received,
            peer_chunks_total: *peer_chunks_total,
        },
        ProtocolState::Verifying => MultiStagePhase::Verifying,
        ProtocolState::Confirming | ProtocolState::Complete | ProtocolState::RetryReady => {
            MultiStagePhase::Confirming
        }
        ProtocolState::Finalized => MultiStagePhase::Finalized {
            peer_name: String::new(),
        },
        ProtocolState::Failed(reason) => MultiStagePhase::Failed {
            reason: reason.clone(),
        },
        // `ProtocolState` is `#[non_exhaustive]`. Any future variant
        // added by vauchi-core defaults to the engine's
        // most-conservative observable phase (Preparing) so the
        // machine surfaces it as "still initializing" rather than
        // misclassifying it into a terminal phase. The T1.1
        // proptest's reach-past-Preparing invariant catches the
        // case where a future variant should actually be
        // post-Preparing — the failure points at this arm.
        _ => MultiStagePhase::Preparing,
    }
}

/// Map a (prior, new) phase pair onto the most informative
/// [`MultiStageEvent`] for the transition. Pure function — the
/// engine integration uses this to decide which screen-handler /
/// command emissions to fire on each transition.
///
/// `None` is returned for no-op transitions (same phase) and for
/// the unusual case where the inner state regresses (which the
/// protocol does not do in practice, but the function is defined
/// for the full Cartesian product to keep the proptest happy).
fn phase_transition_event(prior: &MultiStagePhase, new: &MultiStagePhase) -> MultiStageEvent {
    if prior == new {
        return MultiStageEvent::None;
    }
    match new {
        MultiStagePhase::Discovered => MultiStageEvent::PeerDiscovered,
        MultiStagePhase::Transferring {
            chunks_sent,
            chunks_total,
            chunks_received,
            peer_chunks_total,
        } => MultiStageEvent::TransferProgress {
            chunks_sent: *chunks_sent,
            chunks_total: *chunks_total,
            chunks_received: *chunks_received,
            peer_chunks_total: *peer_chunks_total,
        },
        MultiStagePhase::Verifying => MultiStageEvent::Verifying,
        MultiStagePhase::Confirming => MultiStageEvent::Confirming,
        MultiStagePhase::Finalized { peer_name } => MultiStageEvent::Finalized {
            peer_name: peer_name.clone(),
        },
        MultiStagePhase::Completed => MultiStageEvent::Completed,
        MultiStagePhase::Failed { reason } => MultiStageEvent::Failed {
            reason: reason.clone(),
        },
        // Pre-advertise / advertising / cancelled transitions don't
        // carry a downstream event — the engine reads `phase()` for
        // them directly.
        _ => MultiStageEvent::None,
    }
}

/// Translate a [`MultiStageEvent`] into the set of `Command`s the
/// engine should emit to the frontend. Pure mapping function — the
/// engine integration (T1.2b) calls this from
/// `apply_multi_stage_event`. T1.2 covers the QR display case;
/// T1.2b extends with the screen-presentation trio on phase entry,
/// the audio-handshake commands on `Confirming`, and the
/// restore-defaults trio on terminal exit per T0.2 design.
pub fn event_to_commands(event: &MultiStageEvent) -> Vec<Command> {
    match event {
        MultiStageEvent::QrFrameReady(payload) => vec![Command::QrDisplay {
            data: payload.data.clone(),
        }],
        _ => Vec::new(),
    }
}

/// Read the engine's internal protocol state. Test-only convenience
/// for asserting the inner machine's `ProtocolState` matches the
/// machine's reported phase. Production code never needs this —
/// the phase is the authoritative read.
#[doc(hidden)]
pub fn protocol_state_for_test(machine: &MultiStageMachine) -> ProtocolState {
    machine.inner.get_state()
}
// INLINE_TEST_REQUIRED: decode_peer_name_from_payload is a private
// helper; its edges (short input, wrong version, malformed card_json)
// only exercise here.
#[cfg(test)]
mod peer_name_tests {
    use super::{EXCHANGE_PAYLOAD_VERSION, decode_peer_name_from_payload};
    use vauchi_core::contact_card::ContactCard;

    fn build_payload(card: &ContactCard) -> Vec<u8> {
        let json = serde_json::to_vec(card).expect("serialize card");
        let mut out = Vec::with_capacity(1 + 32 + json.len());
        out.push(EXCHANGE_PAYLOAD_VERSION);
        out.extend_from_slice(&[0xAB; 32]);
        out.extend_from_slice(&json);
        out
    }

    // @internal
    #[test]
    fn well_formed_payload_returns_card_display_name() {
        let card = ContactCard::new("Alice");
        let payload = build_payload(&card);
        assert_eq!(decode_peer_name_from_payload(&payload), "Alice");
    }

    // @internal
    #[test]
    fn empty_payload_returns_empty_string() {
        assert_eq!(decode_peer_name_from_payload(&[]), "");
    }

    // @internal
    #[test]
    fn payload_shorter_than_header_returns_empty_string() {
        let short = vec![EXCHANGE_PAYLOAD_VERSION; 20];
        assert_eq!(decode_peer_name_from_payload(&short), "");
    }

    // @internal
    #[test]
    fn unknown_version_byte_returns_empty_string() {
        let card = ContactCard::new("Bob");
        let mut payload = build_payload(&card);
        payload[0] = 0xFF;
        assert_eq!(decode_peer_name_from_payload(&payload), "");
    }

    // @internal
    #[test]
    fn malformed_card_json_returns_empty_string() {
        let mut payload = vec![EXCHANGE_PAYLOAD_VERSION];
        payload.extend_from_slice(&[0xAB; 32]);
        payload.extend_from_slice(b"{not-valid-json");
        assert_eq!(decode_peer_name_from_payload(&payload), "");
    }
}
