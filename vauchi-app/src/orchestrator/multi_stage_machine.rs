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
//! Phase sequence (extracted from the cycle loop in
//! `vauchi-platform/src/multistage_exchange.rs::cycle_loop`):
//!
//! ```text
//! new ─(no I/O)→ Preparing
//! Preparing ─advance: emit Idle→Advertising→first QR payload─▶ Advertising
//! Advertising ─advance per display_duration_ms tick─▶ Advertising (next QR)
//! Advertising ─QrScanned: peer payload parsed─▶ Discovered
//! Discovered ─advance: chunk transfer started─▶ Transferring{…}
//! Transferring ─advance: protocol verify─▶ Verifying
//! Verifying ─advance: peer ack─▶ Confirming
//! Confirming ─advance: finalize─▶ Finalized {peer_name}
//! Finalized ─advance: cycle-end persistence done─▶ Completed
//! (any) ─HardwareError / PermissionDenied─▶ Failed{reason}
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
//! # T1.1 status (RED)
//!
//! This module exists at the API-stub level so the CC-13 stateful
//! proptest can compile against the surface the GREEN step (T1.2)
//! will fill in. `advance` and `handle_hardware_event` are
//! intentionally minimal (return `MultiStageEvent::None`, leave the
//! phase at `Preparing`) — the proptest catches this as a failing
//! invariant ("a long QrScanned-rich event sequence must reach a
//! post-`Preparing` phase") and turns GREEN once T1.2 wires the
//! inner [`MultiStageSession`] driver.

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
    /// emitted (`now` units). `None` while not in `Advertising`.
    /// Read by T1.2's `advance` when deciding whether the
    /// `display_duration_ms` window has elapsed and the next
    /// frame should be emitted.
    #[allow(dead_code)] // T1.2 wires this; T1.1 RED only.
    current_frame_started_at: Option<u64>,
    /// Per-frame display duration last emitted, in the same `now`
    /// units. `0` while not in `Advertising`. Paired with
    /// `current_frame_started_at` for the per-frame tick check.
    #[allow(dead_code)] // T1.2 wires this; T1.1 RED only.
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
    /// design); the listening window restarts on retry.
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

    /// One non-blocking step. **T1.1 stub** — leaves the phase at
    /// `Preparing` and returns `None`. T1.2 fills in the per-frame
    /// QR emission, peer-discovery observation, finalize-on-cycle-
    /// end behaviour by driving the inner [`MultiStageSession`] and
    /// inspecting its [`ProtocolState`].
    pub fn advance(&mut self, _now: u64) -> MultiStageEvent {
        if self.cancelled || self.is_terminal() {
            return MultiStageEvent::None;
        }
        MultiStageEvent::None
    }

    /// Translate a frontend-emitted [`vauchi_core::Event`] into a
    /// machine transition. **T1.1 stub** — recognises only the
    /// terminal-failure events; T1.2 fills in the `QrScanned` →
    /// `process_scanned_qr` ingress and the audio-samples ingress
    /// for Hover.
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
            Event::HardwareUnavailable { transport } => {
                // Multi-stage tolerates `screen_brightness`,
                // `idle_timer`, `orientation_lock` unavailability
                // (they're best-effort presentation hints). Camera
                // and microphone unavailability are fatal — without
                // them no payload can move.
                match transport.as_str() {
                    "camera" | "microphone" => {
                        let reason = format!("hardware_unavailable:{transport}");
                        self.phase = MultiStagePhase::Failed {
                            reason: reason.clone(),
                        };
                        MultiStageEvent::Failed { reason }
                    }
                    _ => MultiStageEvent::None,
                }
            }
            // Every other Event is inert at the T1.1 stub level.
            // T1.2 wires QrScanned and AudioSamplesRecorded.
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
}

/// Translate a [`MultiStageEvent`] into the set of `Command`s the
/// engine should emit to the frontend. Pure mapping function — the
/// engine integration (T1.2) calls this from
/// `apply_multi_stage_event`. **T1.1 stub** — covers only the
/// `QrFrameReady` case; T1.2 extends with the screen-presentation
/// trio on phase entry, the audio-handshake commands on
/// `Confirming`, and the restore-defaults trio on terminal exit
/// per T0.2 design.
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
/// machine's reported phase. T1.2 production code never needs this —
/// the phase is the authoritative read.
#[doc(hidden)]
pub fn protocol_state_for_test(machine: &MultiStageMachine) -> ProtocolState {
    machine.inner.get_state()
}
