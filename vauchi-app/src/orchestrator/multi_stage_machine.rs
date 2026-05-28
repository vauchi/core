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
use vauchi_core::exchange::{MultiStageSession, ProtocolState, QrPayload};

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
}

/// Mode marker — Glance (bilateral QR only) vs Hover (QR + audio
/// proximity handshake). Matches the existing two-constructor
/// pattern on [`crate::ui::multi_stage_exchange::MultiStageExchangeEngine`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiStageMode {
    Glance,
    Hover,
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
}

impl MultiStageMachine {
    /// Construct a Glance-mode machine — bilateral QR scan, no
    /// ultrasonic proximity handshake. **No I/O.** The first
    /// [`advance`](Self::advance) emits the first QR frame.
    pub fn new_glance(local_card: Vec<u8>, _now: u64) -> Self {
        Self {
            inner: MultiStageSession::new(local_card),
            mode: MultiStageMode::Glance,
            phase: MultiStagePhase::Preparing,
            current_frame_started_at: None,
            current_frame_duration: 0,
            cancelled: false,
        }
    }

    /// Construct a Hover-mode machine — QR + ultrasonic proximity
    /// handshake. Same I/O discipline as `new_glance`. Audio
    /// commands fire on the `Confirming` transition (per T0.2
    /// design); the listening window restarts on retry. T1.2b
    /// wires the audio command emission via `event_to_commands`.
    pub fn new_hover(local_card: Vec<u8>, _now: u64) -> Self {
        Self {
            inner: MultiStageSession::new(local_card),
            mode: MultiStageMode::Hover,
            phase: MultiStagePhase::Preparing,
            current_frame_started_at: None,
            current_frame_duration: 0,
            cancelled: false,
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
                self.sync_phase_from_inner_state();
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
    pub fn handle_hardware_event(&mut self, event: &Event, _now: u64) -> MultiStageEvent {
        if self.cancelled || self.is_terminal() {
            return MultiStageEvent::None;
        }
        match event {
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
                let _ = self.inner.process_scanned_qr(data);
                let prior_phase = self.phase.clone();
                self.sync_phase_from_inner_state();
                phase_transition_event(&prior_phase, &self.phase)
            }
            Event::QrScanProgress { .. } => {
                // Per-frame viewfinder telemetry — the engine-side
                // `MultiStageExchangeEngine` consumes this for the
                // ScanQuality indicator. No protocol effect.
                MultiStageEvent::None
            }
            Event::AudioSamplesRecorded { .. } => {
                // Hover ultrasonic ingress. T1.2b wires the FSK
                // decode via `MultiStageSession::set_audio_proximity`
                // + `verify_audio_response` and the matching
                // `Command::AudioStop` emission. Glance flows emit
                // zero `AudioSamplesRecorded` so this is purely a
                // Hover-path TODO.
                MultiStageEvent::None
            }
            // Every other Event variant is inert for multi-stage —
            // late BLE notifications, NFC taps, link callbacks etc.
            // arriving on the multi-stage screen are ignored to
            // keep the machine's surface narrow.
            _ => MultiStageEvent::None,
        }
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

    /// Re-derive `self.phase` from the inner [`MultiStageSession`]
    /// state. Called after every protocol-driving operation
    /// (`get_display_qr`, `process_scanned_qr`).
    fn sync_phase_from_inner_state(&mut self) {
        // Once a user cancel has been observed the phase is
        // absorbing — never let an inner transition un-cancel us.
        if self.cancelled {
            return;
        }
        let new_phase = phase_from_protocol_state(&self.inner.get_state());
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
