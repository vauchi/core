// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the multi-stage exchange session.
//!
//! Wraps [`MultiStageSession`] in a thread-safe handle (`Mutex`) and exposes
//! mobile-friendly enums/records for state and QR payloads.
//!
//! # Lifecycle (ADR-031 + G4 event API)
//!
//! ```text
//! let session = VauchiPlatform::create_multistage_session()?;   // or ::new(card) in tests
//! session.set_listener(Box::new(my_listener));
//! session.start();                                              // spawns cycle thread
//! // while the camera is scanning:
//! session.process_scanned_qr(scanned_data);                     // may be called concurrently
//! // on leaving the screen / error:
//! session.cancel();                                             // idempotent; joins cycle thread
//! ```
//!
//! The cycle thread owns the protocol clock — frontends must **not** run their
//! own timers. All state progress arrives via [`MultiStageSessionListener`]
//! callbacks from the `vauchi-exchange-cycle` thread. Consumers must dispatch
//! to their UI thread before touching UI state.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::error::{LOCK_POISON_MSG, MobileError};
use crate::mobile_exchange::deserialize_exchange_payload;

use vauchi_core::Command;
use vauchi_core::contact::Contact;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::{
    AudioConfig, MultiStageSession, ProtocolState, QrPayload, audio_modem,
};
use vauchi_core::storage::Storage;

/// Mobile-friendly protocol state enum (UniFFI-compatible).
#[derive(uniffi::Enum, Debug, Clone, PartialEq)]
pub enum MobileProtocolState {
    Idle,
    Advertising,
    Discovered,
    Transferring {
        chunks_sent: u16,
        chunks_total: u16,
        chunks_received: u16,
        peer_chunks_total: u16,
    },
    Verifying,
    Confirming,
    Complete,
    Finalized,
    Failed {
        reason: String,
    },
}

impl From<ProtocolState> for MobileProtocolState {
    fn from(state: ProtocolState) -> Self {
        match state {
            ProtocolState::Idle => MobileProtocolState::Idle,
            ProtocolState::Advertising => MobileProtocolState::Advertising,
            ProtocolState::Discovered => MobileProtocolState::Discovered,
            ProtocolState::Transferring {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            } => MobileProtocolState::Transferring {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            },
            ProtocolState::Verifying => MobileProtocolState::Verifying,
            ProtocolState::Confirming => MobileProtocolState::Confirming,
            ProtocolState::Complete | ProtocolState::RetryReady => MobileProtocolState::Complete,
            ProtocolState::Finalized => MobileProtocolState::Finalized,
            ProtocolState::Failed(reason) => MobileProtocolState::Failed { reason },
            _ => MobileProtocolState::Idle,
        }
    }
}

/// QR payload for mobile display.
#[derive(uniffi::Record, Debug, Clone)]
pub struct MobileQrPayload {
    pub data: String,
    pub error_correction: String,
    pub display_duration_ms: u32,
}

impl From<QrPayload> for MobileQrPayload {
    fn from(qr: QrPayload) -> Self {
        MobileQrPayload {
            data: qr.data,
            error_correction: qr.error_correction,
            display_duration_ms: qr.display_duration_ms,
        }
    }
}

/// Push-based callback interface for multi-stage exchange events.
///
/// Frontends implement this trait (in Swift/Kotlin via UniFFI) and register
/// it with [`MobileMultiStageSession::set_listener`] before calling
/// [`MobileMultiStageSession::start`]. Once `start()` is called, an internal
/// `vauchi-exchange-cycle` thread drives the protocol clock and invokes these
/// callbacks as state advances.
///
/// # Threading
///
/// Callbacks fire from the cycle thread, **not** the main/UI thread.
/// Consumers must marshal to their platform's UI thread before touching UI
/// state (`DispatchQueue.main.async` on iOS, `withContext(Dispatchers.Main)`
/// on Android).
///
/// # Callback contract
///
/// - [`on_qr_payload`](Self::on_qr_payload) — fires for every QR frame the
///   frontend should render.
/// - [`on_state_changed`](Self::on_state_changed) — fires only when the
///   protocol state actually changes (no duplicates).
/// - [`on_finalized`](Self::on_finalized) — fires exactly once per successful
///   session, carries the peer's display name for UX.
/// - [`on_session_ended`](Self::on_session_ended) — final callback on the
///   session (grace expired, FAIL broadcast done, or cancelled). Always last.
#[uniffi::export(callback_interface)]
pub trait MultiStageSessionListener: Send + Sync {
    /// New QR payload to render. Core handles the cycle timing; the frontend
    /// just renders whatever this emits and stops when the next callback
    /// arrives.
    fn on_qr_payload(&self, payload: MobileQrPayload);

    /// Protocol state changed. Fires once per actual transition.
    fn on_state_changed(&self, state: MobileProtocolState);

    /// Exchange finalized successfully. `contact_name` is the peer's card
    /// display name. Fires exactly once per successful session, before
    /// [`on_session_ended`](Self::on_session_ended).
    fn on_finalized(&self, contact_name: String);

    /// Session has fully ended — grace period expired, FAIL broadcast
    /// completed, or `cancel()` was called. Always the last callback.
    fn on_session_ended(&self);
}

/// Mobile-facing audio proximity state. UniFFI-exposed mirror of
/// [`vauchi_core::exchange::AudioProximityState`] (kept as a sibling
/// enum so the wire shape is independent of the core internal type,
/// matching the [`MobileProtocolState`] / [`ProtocolState`] pattern).
///
/// Glance never transitions out of `Pending`; Hover walks
/// `Pending → Listening → Confirmed` on success or
/// `Pending → Listening → Failed` on the proximity timeout. Phase
/// 1.C.3c plumbing — the wrapper-side orchestrator (Phase 1.C.3d)
/// drives transitions via `MultiStageSession::set_audio_proximity`
/// and surfaces them via [`MultiStageAudioListener::on_audio_state_changed`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MobileAudioProximityState {
    Pending,
    Listening,
    Confirmed,
    Failed,
}

impl From<vauchi_core::exchange::AudioProximityState> for MobileAudioProximityState {
    fn from(state: vauchi_core::exchange::AudioProximityState) -> Self {
        use vauchi_core::exchange::AudioProximityState::*;
        match state {
            Pending => Self::Pending,
            Listening => Self::Listening,
            Confirmed => Self::Confirmed,
            Failed => Self::Failed,
        }
    }
}

/// Audio-proximity-only listener for Hover ultrasonic handshake state
/// updates.
///
/// Sibling of [`MultiStageSessionListener`] rather than an extension,
/// so adding the audio callback does not break existing Swift / Kotlin
/// consumers of the base listener. Mobile clients that don't care about
/// audio (Glance-only flows, headless tooling) simply don't register
/// one and the audio path stays inert.
///
/// Phase 1.C.3d wires the wrapper-side orchestrator (ProximityVerifier
/// invocation + `Command::AudioEmitChallenge` / `AudioListenForResponse`
/// emission) that calls
/// [`on_audio_state_changed`](Self::on_audio_state_changed) whenever
/// the inner [`MultiStageSession::set_audio_proximity`] transition
/// succeeds. Until then, this trait + the slot below are dormant
/// plumbing — register a listener, but no callbacks fire yet.
#[uniffi::export(callback_interface)]
pub trait MultiStageAudioListener: Send + Sync {
    /// The session's audio-proximity state changed. Fires once per
    /// real transition (the wrapper preflights the state graph so
    /// rejected transitions never reach this callback).
    fn on_audio_state_changed(&self, state: MobileAudioProximityState);
}

type AudioListenerSlot = Arc<Mutex<Option<Arc<dyn MultiStageAudioListener>>>>;

/// Fallback sleep duration (ms) when a QR payload does not carry a hint.
///
/// Matches the protocol's minimum `DISPLAY_MS_INIT` so the cycle thread
/// stays responsive to cancellation.
const DEFAULT_CYCLE_SLEEP_MS: u32 = 400;

type ListenerSlot = Arc<Mutex<Option<Arc<dyn MultiStageSessionListener>>>>;

/// Persistence handle the cycle thread uses to land the peer contact in
/// storage when the protocol reaches `Finalized`. Cheaply cloned (a path
/// string + the storage key) so each `start()` call can move a copy into
/// the cycle thread without sharing state with the session struct.
///
/// `None` for harness/test sessions constructed via
/// [`MobileMultiStageSession::new`]; `Some` for production sessions
/// constructed via [`VauchiPlatform::create_multistage_session`].
#[derive(Clone)]
pub(crate) struct PersistenceContext {
    pub(crate) storage_path: PathBuf,
    pub(crate) storage_key: SymmetricKey,
}

/// Multi-stage exchange session handle for mobile platforms.
///
/// Holds the protocol state machine plus the event-cycle thread that drives
/// it. `listener` is an `Arc<Mutex<…>>` so the cycle thread and
/// `set_listener` share one slot — rebinds mid-session propagate immediately.
#[derive(uniffi::Object)]
pub struct MobileMultiStageSession {
    inner: Arc<Mutex<MultiStageSession>>,
    listener: ListenerSlot,
    /// Phase 1.C.3c plumbing — sibling listener for audio-proximity
    /// state changes. `None` for sessions that don't care about audio
    /// (Glance-only flows, headless tooling).
    audio_listener: AudioListenerSlot,
    cancel_flag: Arc<AtomicBool>,
    thread_handle: Mutex<Option<JoinHandle<()>>>,
    /// Test-only sleep override (ms). `0` means "use the payload's
    /// `display_duration_ms`" (production). Non-zero forces every cycle
    /// iteration to sleep this many ms regardless of the payload hint — used
    /// by the test harness to drive an exchange at wall-clock speeds that fit
    /// inside a unit test.
    cycle_sleep_override_ms: Arc<AtomicU32>,
    /// Storage handle for the cycle thread's Finalized-state persistence
    /// path. `Some` for sessions created from a `VauchiPlatform`, `None`
    /// for the harness constructor used by listener-only unit tests.
    persistence: Option<PersistenceContext>,
    /// Phase 1.C.3e-ii — commands the orchestrator emits as a side
    /// effect of audio-handshake state transitions. The consumer
    /// (PlatformAppEngine) drains this queue right after each audio
    /// state-change callback and forwards into AppEngine's
    /// pending-command stream so the frontend sees
    /// `Command::AudioEmitChallenge` / `AudioListenForResponse` in
    /// the next `screen_envelope_to_json` drain.
    pending_audio_commands: Arc<Mutex<Vec<Command>>>,
}

#[uniffi::export]
impl MobileMultiStageSession {
    /// Create a new session for the given local contact card payload.
    ///
    /// The session starts in `Idle` with no listener attached. The caller
    /// must:
    ///
    /// 1. Register a listener via [`set_listener`](Self::set_listener).
    /// 2. Call [`start`](Self::start) to spawn the cycle thread.
    /// 3. Feed camera scans via
    ///    [`process_scanned_qr`](Self::process_scanned_qr).
    /// 4. Call [`cancel`](Self::cancel) when leaving the exchange view.
    ///
    /// `local_card` is the raw payload the protocol will transfer — normally
    /// produced by `VauchiPlatform::create_multistage_session`.
    #[uniffi::constructor]
    pub fn new(local_card: Vec<u8>) -> Self {
        MobileMultiStageSession::build(local_card, None)
    }

    /// Register (or replace) the event listener. Safe to call before or
    /// after [`start`](Self::start); subsequent callbacks route to the most
    /// recently installed listener.
    pub fn set_listener(&self, listener: Box<dyn MultiStageSessionListener>) {
        if let Ok(mut slot) = self.listener.lock() {
            *slot = Some(Arc::from(listener));
        }
    }

    /// Register (or replace) the audio-proximity listener. Optional —
    /// sessions that don't register one simply never receive audio
    /// callbacks (Glance flows, harness tests). Phase 1.C.3d wires
    /// the wrapper-side orchestrator that fires this callback;
    /// until then, registering a listener is harmless dormant
    /// plumbing.
    pub fn set_audio_listener(&self, listener: Box<dyn MultiStageAudioListener>) {
        if let Ok(mut slot) = self.audio_listener.lock() {
            *slot = Some(Arc::from(listener));
        }
    }

    /// Returns the current audio-proximity state of the inner session.
    /// Hover sessions transition through the state machine driven by
    /// `MultiStageSession::set_audio_proximity`; Glance sessions stay
    /// at `Pending` for their lifetime.
    pub fn audio_proximity(&self) -> MobileAudioProximityState {
        self.inner
            .lock()
            .map(|s| s.audio_proximity().into())
            .unwrap_or(MobileAudioProximityState::Pending)
    }

    /// Spawn the cycle thread. Idempotent — a second call while the thread
    /// is running is a no-op. Requires [`set_listener`](Self::set_listener)
    /// to have been called; without a listener the thread still runs but
    /// drops every event.
    pub fn start(&self) {
        let Ok(mut handle_slot) = self.thread_handle.lock() else {
            return;
        };
        if handle_slot.as_ref().is_some_and(|h| !h.is_finished()) {
            return;
        }
        // Clear any stale handle from a prior completed run.
        *handle_slot = None;

        // Reset cancellation for this run.
        self.cancel_flag.store(false, Ordering::Relaxed);

        let inner = Arc::clone(&self.inner);
        let listener = Arc::clone(&self.listener);
        let audio_listener = Arc::clone(&self.audio_listener);
        let audio_commands = Arc::clone(&self.pending_audio_commands);
        let cancel_flag = Arc::clone(&self.cancel_flag);
        let sleep_override = Arc::clone(&self.cycle_sleep_override_ms);
        let persistence = self.persistence.clone();

        let spawn_result = thread::Builder::new()
            .name("vauchi-exchange-cycle".into())
            .spawn(move || {
                cycle_loop(
                    inner,
                    listener,
                    audio_listener,
                    audio_commands,
                    cancel_flag,
                    sleep_override,
                    persistence,
                );
            });
        if let Ok(handle) = spawn_result {
            *handle_slot = Some(handle);
        }
    }

    /// Feed a scanned QR string into the protocol engine.
    ///
    /// Safe to call concurrently with the cycle thread — the inner
    /// `MultiStageSession` is serialized by the same `Mutex` both paths
    /// hold. Returns the post-scan state so the camera pipeline has an
    /// immediate signal even before the next listener cycle observes the
    /// transition.
    pub fn process_scanned_qr(&self, raw: String) -> MobileProtocolState {
        let Ok(mut session) = self.inner.lock() else {
            return MobileProtocolState::Failed {
                reason: LOCK_POISON_MSG.into(),
            };
        };
        session.process_scanned_qr(&raw).into()
    }

    /// Cancel the session. Sets the cancellation flag, waits for the cycle
    /// thread to exit, wipes sensitive state, and drops the registered
    /// listener. Idempotent — safe to call before `start`, multiple times,
    /// or from any thread.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::Relaxed);

        // Wipe sensitive protocol state.
        if let Ok(mut session) = self.inner.lock() {
            session.cancel();
        }

        // Join the cycle thread outside the inner lock so a mid-flight
        // iteration can complete and observe the cancel flag.
        let handle_opt = self
            .thread_handle
            .lock()
            .ok()
            .and_then(|mut slot| slot.take());
        if let Some(handle) = handle_opt {
            let _ = handle.join();
        }

        // Drop the listener last so a concurrent `cancel()` racing with a
        // final `on_session_ended` still observes a coherent state.
        if let Ok(mut slot) = self.listener.lock() {
            *slot = None;
        }
    }
}

impl MobileMultiStageSession {
    /// Drive the inner session's audio-proximity state machine
    /// through the `Listening` transition and notify the audio
    /// listener.
    ///
    /// `pub(crate)` because this is an internal trigger called by
    /// the wrapper-side orchestrator (Phase 1.C.3e-ii) when the
    /// multi-stage session reaches the appropriate protocol moment.
    /// UniFFI consumers (iOS / Android) do not call this directly —
    /// the orchestrator decides timing. Exposed for tests so the
    /// trigger pattern is verifiable independently of the future
    /// orchestrator wiring.
    ///
    /// Returns `Ok(())` if the transition is allowed by the inner
    /// state graph (Pending → Listening, or Failed → Listening for
    /// the retry path per G1.3 of the Hover graduation problem
    /// record). Returns [`AudioStateError`] otherwise.
    ///
    /// Phase 1.C.3e-ii will extend this method to also generate the
    /// FSK challenge waveform via
    /// `vauchi_core::exchange::audio_modem` and push
    /// `Command::AudioEmitChallenge` /
    /// `Command::AudioListenForResponse` into the orchestrator's
    /// command queue. Today the method only flips state + notifies;
    /// the frontend won't hear or play a chirp until 1.C.3e-ii.
    /// Audio-listen window default — mirrors the legacy
    /// `ExchangeSession::Qr` Hover path (`vauchi-core/src/exchange/session.rs:500`).
    /// Phase 1.C.4 will move this behind the Clock seam so tests can
    /// drive it deterministically.
    pub(crate) const AUDIO_LISTEN_TIMEOUT_MS: u64 = 5000;

    pub fn start_audio_handshake(
        &self,
        challenge: &[u8; 16],
    ) -> Result<(), vauchi_core::exchange::AudioStateError> {
        let result = self
            .inner
            .lock()
            .map_err(
                |_| vauchi_core::exchange::AudioStateError::InvalidTransition {
                    from: vauchi_core::exchange::AudioProximityState::Pending,
                    to: vauchi_core::exchange::AudioProximityState::Listening,
                },
            )?
            .set_audio_proximity(vauchi_core::exchange::AudioProximityState::Listening);
        if result.is_ok() {
            // Generate the FSK challenge waveform from the peer's
            // shared challenge bytes (extracted from the multi-stage
            // INIT QR by the orchestrator — Phase 1.C.3e-iii). The
            // mobile audio backend plays the samples + records the
            // response; core verifies via `audio_modem::decode_fsk_samples`
            // (Phase 1.C.3e-iv).
            let config = AudioConfig::default();
            let samples = audio_modem::generate_fsk_samples(challenge, &config);
            let sample_rate = config.sample_rate;
            let cmds = vec![
                Command::AudioEmitChallenge {
                    samples,
                    sample_rate,
                },
                Command::AudioListenForResponse {
                    timeout_ms: Self::AUDIO_LISTEN_TIMEOUT_MS,
                    sample_rate,
                },
            ];
            if let Ok(mut queue) = self.pending_audio_commands.lock() {
                queue.extend(cmds);
            }
            self.notify_audio_listener(MobileAudioProximityState::Listening);
        }
        result
    }

    /// Convenience wrapper around [`Self::start_audio_handshake`] that
    /// derives the challenge bytes from the inner session's
    /// [`MultiStageSession::session_id`]. The orchestrator calls this
    /// when the multi-stage protocol reaches the right moment (likely
    /// the `Discovered → Transferring` boundary — Phase 1.C.3e-v
    /// wires the PlatformAppEngine trigger). Tests + the future
    /// autonomous trigger inside the cycle thread are the only other
    /// callers.
    ///
    /// **Challenge derivation** (Phase 1.C.3e-iii temporary): emits
    /// our own `session_id` as the FSK payload. Both peers know each
    /// other's session_id after Stage 1, so each side's mic decode
    /// can compare against `peer_session_id` and verify the audio
    /// channel really carried it from the right peer. Phase 1.C.4
    /// may swap this for a dedicated `audio_challenge` field on the
    /// INIT QR payload — see the doc on
    /// [`MultiStageSession::session_id`].
    pub fn start_audio_handshake_for_session(
        &self,
    ) -> Result<(), vauchi_core::exchange::AudioStateError> {
        let challenge = self
            .inner
            .lock()
            .map_err(
                |_| vauchi_core::exchange::AudioStateError::InvalidTransition {
                    from: vauchi_core::exchange::AudioProximityState::Pending,
                    to: vauchi_core::exchange::AudioProximityState::Listening,
                },
            )?
            .session_id();
        self.start_audio_handshake(&challenge)
    }

    /// Process audio samples recorded by the platform audio backend
    /// in response to a `Command::AudioListenForResponse`. Decodes
    /// the FSK waveform via [`audio_modem::decode_fsk_samples`],
    /// compares the decoded bytes against the inner session's
    /// `peer_session_id` using constant-time equality, and
    /// transitions the audio-proximity state.
    ///
    /// Outcome:
    /// - Decode succeeds AND decoded bytes match the peer's
    ///   session_id → transition Listening → Confirmed. The audio
    ///   channel verifiably carried the peer's identifier to our
    ///   mic, satisfying the Hover physical-proximity claim.
    /// - Decode succeeds but bytes mismatch → Listening → Failed.
    ///   Some other audio source was emitting in the ultrasonic
    ///   band; we didn't hear the peer.
    /// - Decode fails (malformed samples, preamble not found,
    ///   resampling error) → Listening → Failed. Mic didn't pick up
    ///   the chirp clearly enough.
    /// - peer_session_id is None (Stage 1 not yet complete) →
    ///   Listening → Failed. Caller raced; orchestrator should have
    ///   waited for Discovered before triggering the handshake.
    ///
    /// Returns the inner [`AudioStateError`] if the state-machine
    /// rejected the transition (e.g. called outside the Listening
    /// window). Notifies the audio listener with the new state on
    /// success.
    ///
    /// **Security**: constant-time comparison via
    /// [`subtle::ConstantTimeEq`] so the verification doesn't leak
    /// timing about which byte differed. The mismatch class
    /// (Failed) is the same regardless of WHERE the mismatch is,
    /// preventing a near-peer attacker from probing the decoded
    /// bytes via timing.
    pub fn process_audio_samples_recorded(
        &self,
        samples: Vec<f32>,
        sample_rate: u32,
    ) -> Result<(), vauchi_core::exchange::AudioStateError> {
        use vauchi_core::exchange::{AudioConfig, AudioProximityState, audio_modem};

        // Decode is CPU-bound; do it without holding the inner lock
        // so concurrent state callbacks (e.g. on_state_changed during
        // the listen window) aren't blocked.
        let config = AudioConfig::default();
        let decoded = audio_modem::decode_fsk_samples(&samples, sample_rate, &config);

        let new_state = match decoded {
            Ok(decoded_bytes) => {
                // Constant-time verification lives in vauchi-core
                // (subtle is a core dep). `None` from
                // verify_audio_response means peer_session_id wasn't
                // set yet (Stage 1 incomplete) — user-facing UX is
                // the same Failed; orchestrator distinguishes via
                // logs if needed.
                let verified = self
                    .inner
                    .lock()
                    .ok()
                    .and_then(|g| g.verify_audio_response(&decoded_bytes));
                if verified == Some(true) {
                    AudioProximityState::Confirmed
                } else {
                    AudioProximityState::Failed
                }
            }
            // Decode error (preamble not found, malformed samples,
            // resampling failure) → Failed. User-facing UX same as
            // verification failure ("couldn't confirm devices are
            // close").
            Err(_) => AudioProximityState::Failed,
        };

        let result = self
            .inner
            .lock()
            .map_err(
                |_| vauchi_core::exchange::AudioStateError::InvalidTransition {
                    from: AudioProximityState::Listening,
                    to: new_state,
                },
            )?
            .set_audio_proximity(new_state);
        if result.is_ok() {
            self.notify_audio_listener(new_state.into());
        }
        result
    }

    /// Drain the queue of audio commands produced by
    /// [`Self::start_audio_handshake`] (and future orchestrator
    /// transitions). Returns the commands in emission order and
    /// empties the queue. `PlatformAppEngine` calls this after
    /// processing each audio state-change callback so the commands
    /// reach AppEngine's pending-command stream.
    pub fn drain_audio_commands(&self) -> Vec<Command> {
        self.pending_audio_commands
            .lock()
            .map(|mut q| std::mem::take(&mut *q))
            .unwrap_or_default()
    }

    /// Returns an `Arc` handle to the pending audio commands queue
    /// so PlatformAppEngine's audio-listener bridge can drain
    /// commands from inside its `on_audio_state_changed` callback
    /// (Phase 1.C.3e-v). The bridge cannot reach `self` directly
    /// without creating an Arc cycle through the listener slot;
    /// holding the queue Arc separately avoids the cycle.
    ///
    /// Internal — non-UniFFI. Frontends never call this.
    pub fn pending_audio_commands_handle(&self) -> Arc<Mutex<Vec<Command>>> {
        Arc::clone(&self.pending_audio_commands)
    }

    /// Pop the audio listener slot, clone the Arc out of the lock,
    /// fire the callback unlocked. Matches the lock-free callback
    /// discipline used by [`MultiStageEngineBridge::notify`] in
    /// `platform_app_engine.rs` so a frontend implementation that
    /// re-enters Rust on the callback (typical: read
    /// `current_screen_json`) cannot deadlock.
    pub fn notify_audio_listener(&self, state: MobileAudioProximityState) {
        let listener = self
            .audio_listener
            .lock()
            .ok()
            .and_then(|guard| guard.clone());
        if let Some(listener) = listener {
            listener.on_audio_state_changed(state);
        }
    }

    /// Internal constructor used by `VauchiPlatform::create_multistage_session`
    /// to attach the persistence context the cycle thread needs to land the
    /// peer contact in storage at the `Finalized` transition.
    pub(crate) fn with_persistence(
        local_card: Vec<u8>,
        storage_path: PathBuf,
        storage_key: SymmetricKey,
    ) -> Self {
        MobileMultiStageSession::build(
            local_card,
            Some(PersistenceContext {
                storage_path,
                storage_key,
            }),
        )
    }

    fn build(local_card: Vec<u8>, persistence: Option<PersistenceContext>) -> Self {
        MobileMultiStageSession {
            inner: Arc::new(Mutex::new(MultiStageSession::new(local_card))),
            listener: Arc::new(Mutex::new(None)),
            audio_listener: Arc::new(Mutex::new(None)),
            pending_audio_commands: Arc::new(Mutex::new(Vec::new())),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: Mutex::new(None),
            cycle_sleep_override_ms: Arc::new(AtomicU32::new(0)),
            persistence,
        }
    }

    /// Integration-test hook: force the cycle thread to sleep for a fixed
    /// duration each iteration instead of honouring the payload's
    /// `display_duration_ms`. Set to `0` to restore production behaviour.
    ///
    /// Exposed as `pub` (rather than `pub(crate)`) because integration
    /// tests live in a separate crate. Not part of the UniFFI surface —
    /// the `_for_test` suffix is the load-bearing contract with callers.
    #[doc(hidden)]
    pub fn set_cycle_sleep_override_ms_for_test(&self, override_ms: u32) {
        self.cycle_sleep_override_ms
            .store(override_ms, Ordering::Relaxed);
    }

    /// Integration-test hook: returns true if the cycle thread has exited.
    #[doc(hidden)]
    pub fn cycle_thread_finished_for_test(&self) -> bool {
        self.thread_handle
            .lock()
            .ok()
            .and_then(|slot| slot.as_ref().map(|h| h.is_finished()))
            .unwrap_or(true)
    }
}

/// Body of the cycle thread. Runs until `cancel_flag` flips or the inner
/// session reports no further QR to display (natural termination after the
/// finalized grace period or FAIL broadcast window).
fn cycle_loop(
    inner: Arc<Mutex<MultiStageSession>>,
    listener_slot: ListenerSlot,
    audio_listener_slot: AudioListenerSlot,
    audio_commands: Arc<Mutex<Vec<Command>>>,
    cancel_flag: Arc<AtomicBool>,
    sleep_override_ms: Arc<AtomicU32>,
    persistence: Option<PersistenceContext>,
) {
    let mut prev_state: Option<MobileProtocolState> = None;
    let mut finalized_fired = false;

    loop {
        if cancel_flag.load(Ordering::Relaxed) {
            break;
        }

        let step = {
            let Ok(mut session) = inner.lock() else {
                break;
            };
            let qr = session.get_display_qr();
            let after_state: MobileProtocolState = session.get_state().into();
            let entering_finalized =
                matches!(after_state, MobileProtocolState::Finalized) && !finalized_fired;
            let received_for_finalize = entering_finalized
                .then(|| session.get_received_data())
                .flatten();
            let transport_key_for_finalize = entering_finalized
                .then(|| session.get_transport_key())
                .flatten();
            CycleStep {
                qr_payload: qr.map(MobileQrPayload::from),
                new_state: after_state,
                finalize_payload: received_for_finalize,
                finalize_transport_key: transport_key_for_finalize,
            }
        };

        let transitioned_to = match &prev_state {
            Some(prev) if prev != &step.new_state => Some(step.new_state.clone()),
            None => Some(step.new_state.clone()),
            _ => None,
        };

        let listener = listener_slot.lock().ok().and_then(|g| g.clone());

        if let (Some(listener), Some(payload)) = (listener.as_ref(), step.qr_payload.as_ref()) {
            listener.on_qr_payload(payload.clone());
        }

        if let (Some(listener), Some(state)) = (listener.as_ref(), transitioned_to.as_ref()) {
            listener.on_state_changed(state.clone());
        }

        // Phase 1.C.3e-vi: autonomous audio-handshake trigger. When
        // the multi-stage protocol transitions into `Discovered`
        // (peer scanned our INIT QR + we scanned theirs, transport
        // key derived), fire the ultrasonic handshake without
        // requiring an external orchestrator. The handshake is
        // idempotent — if audio_proximity is already past Pending
        // (Listening / Confirmed / Failed) the inner state machine
        // rejects the transition and we skip the queue / listener
        // notification silently.
        //
        // Glance flows reach Discovered too, but the inner
        // `set_audio_proximity(Listening)` call goes through the
        // state-machine gate regardless of mode — Glance just never
        // *uses* the resulting Listening state because no consumer
        // wires the audio listener. For now the Listening transition
        // fires for all multi-stage flows; Phase 1.E's mode-dispatch
        // flip will route Glance to a different engine entirely
        // (no audio listener path) so the spurious transition is
        // structurally inert.
        if matches!(
            transitioned_to.as_ref(),
            Some(
                MobileProtocolState::Discovered
                    | MobileProtocolState::Transferring { .. }
                    | MobileProtocolState::Verifying
                    | MobileProtocolState::Confirming
                    | MobileProtocolState::Complete
                    | MobileProtocolState::Finalized
            )
        ) {
            try_autonomous_audio_trigger(&inner, &audio_listener_slot, &audio_commands);
        }

        if !finalized_fired
            && matches!(step.new_state, MobileProtocolState::Finalized)
            && let Some(ref payload) = step.finalize_payload
        {
            let name = contact_name_from_payload(payload);

            // Persist the peer contact *before* notifying the frontend.
            // If persistence is configured (production path) and fails,
            // surface a Failed state instead of a misleading on_finalized.
            // Sessions without persistence (harness/listener unit tests)
            // still fire on_finalized as before.
            let persist_result = persistence.as_ref().map(|ctx| {
                let transport_key =
                    step.finalize_transport_key
                        .ok_or_else(|| MobileError::Other {
                            detail: "Finalized without transport key — cannot persist contact"
                                .to_string(),
                        })?;
                persist_finalized_contact(ctx, payload, transport_key)
            });

            match persist_result {
                None | Some(Ok(())) => {
                    if let Some(listener) = listener.as_ref() {
                        listener.on_finalized(name);
                    }
                }
                Some(Err(err)) => {
                    if let Some(listener) = listener.as_ref() {
                        listener.on_state_changed(MobileProtocolState::Failed {
                            reason: format!("persistence failed: {err}"),
                        });
                    }
                }
            }

            finalized_fired = true;
        }

        prev_state = Some(step.new_state.clone());

        if step.qr_payload.is_none() {
            break;
        }

        let sleep_ms = match sleep_override_ms.load(Ordering::Relaxed) {
            0 => step
                .qr_payload
                .as_ref()
                .map(|p| p.display_duration_ms)
                .unwrap_or(DEFAULT_CYCLE_SLEEP_MS),
            override_ms => override_ms,
        };
        responsive_sleep(Duration::from_millis(sleep_ms as u64), &cancel_flag);
    }

    // Final callback — fires once per session on any exit path: cancel,
    // natural termination (grace expired / FAIL broadcast complete), or a
    // poisoned inner lock. Callers rely on this as the "session cleanup
    // complete" signal.
    if let Some(listener) = listener_slot.lock().ok().and_then(|g| g.clone()) {
        listener.on_session_ended();
    }
}
/// Phase 1.C.3e-vi helper: fire the Hover audio handshake autonomously
/// when the cycle thread observes a transition into `Discovered`. Reads
/// the inner session's `session_id` to use as the FSK challenge
/// (mirrors [`MobileMultiStageSession::start_audio_handshake_for_session`]
/// but doesn't require `&self` — operates on the Arc clones the cycle
/// thread already holds).
///
/// Silent no-op when:
/// - no audio listener has been registered (the canonical "not a
///   Hover session" signal — Glance flows and headless tooling
///   never call `set_audio_listener`, so the slot is empty and
///   the cycle thread must not advance the state machine into
///   `Listening` or surface audio commands to the renderer)
/// - the inner mutex is poisoned (don't crash the cycle thread)
/// - the inner state machine rejects the transition (handshake already
///   started, or audio_proximity isn't Pending — Glance retry races,
///   Failed→Listening retry without the orchestrator firing yet, etc.)
///
/// Same lock-free callback discipline as
/// [`MobileMultiStageSession::notify_audio_listener`] — clone the
/// listener Arc out of its slot before invoking so a frontend that
/// re-enters Rust on the callback cannot deadlock with the inner
/// session lock.
fn try_autonomous_audio_trigger(
    inner: &Arc<Mutex<MultiStageSession>>,
    audio_listener_slot: &AudioListenerSlot,
    audio_commands: &Arc<Mutex<Vec<Command>>>,
) {
    use vauchi_core::exchange::{AudioConfig, AudioProximityState, audio_modem};

    // Mode gate: callers that don't register an audio listener
    // (Glance flows, headless tooling) must not see audio
    // commands surface to the renderer or have the inner state
    // machine advance to Listening. The PAE wire-up registers
    // this listener only for Hover-mode engines (Phase 1.C polish),
    // so an empty slot is the canonical "not a Hover session"
    // signal at the cycle-thread layer.
    let listener_registered = audio_listener_slot
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false);
    if !listener_registered {
        return;
    }

    // Acquire inner, check Pending, capture session_id, transition
    // to Listening. Drop lock before queuing / notifying so a
    // listener callback cannot deadlock with later inner locks.
    let (challenge, transitioned) = {
        let Ok(mut session) = inner.lock() else {
            return;
        };
        if session.audio_proximity() != AudioProximityState::Pending {
            return;
        }
        let challenge = session.session_id();
        let result = session.set_audio_proximity(AudioProximityState::Listening);
        (challenge, result.is_ok())
    };
    if !transitioned {
        return;
    }

    // Generate FSK + push paired commands. Mirrors the body of
    // start_audio_handshake from 1.C.3e-ii.
    let config = AudioConfig::default();
    let samples = audio_modem::generate_fsk_samples(&challenge, &config);
    let sample_rate = config.sample_rate;
    let cmds = vec![
        Command::AudioEmitChallenge {
            samples,
            sample_rate,
        },
        Command::AudioListenForResponse {
            timeout_ms: MobileMultiStageSession::AUDIO_LISTEN_TIMEOUT_MS,
            sample_rate,
        },
    ];
    if let Ok(mut queue) = audio_commands.lock() {
        queue.extend(cmds);
    }

    // Lock-free audio listener notification.
    let listener = audio_listener_slot
        .lock()
        .ok()
        .and_then(|guard| guard.clone());
    if let Some(listener) = listener {
        listener.on_audio_state_changed(MobileAudioProximityState::Listening);
    }
}

struct CycleStep {
    qr_payload: Option<MobileQrPayload>,
    new_state: MobileProtocolState,
    finalize_payload: Option<Vec<u8>>,
    finalize_transport_key: Option<[u8; 32]>,
}

/// Persist the peer contact + initialise the double ratchet.
///
/// Mirrors the body that previously lived in
/// `VauchiPlatform::finalize_multistage_exchange` (removed in Phase 3
/// partial alongside the deprecated polling getters): deserialize the
/// exchange payload, build a `Contact`, upsert via `save_contact`, then
/// initialise the ratchet keyed off the transport secret. Runs on the
/// cycle thread, so any error here stays local to that thread and is
/// reported through `on_state_changed(Failed{…})`.
fn persist_finalized_contact(
    ctx: &PersistenceContext,
    received_data: &[u8],
    transport_key: [u8; 32],
) -> Result<(), MobileError> {
    let (public_key, card) = deserialize_exchange_payload(received_data)?;
    let shared_key = SymmetricKey::from_bytes(transport_key);
    let contact = Contact::from_exchange(public_key, card, shared_key.clone());

    let storage =
        Storage::open(&ctx.storage_path, ctx.storage_key.clone()).map_err(MobileError::from)?;
    let contact_id = contact.id().to_string();

    storage.save_contact(&contact)?;

    let ratchet =
        DoubleRatchetState::initialize_initiator(&shared_key, public_key).map_err(|e| {
            MobileError::Other {
                detail: e.to_string(),
            }
        })?;
    storage.save_ratchet_state(&contact_id, &ratchet, true)?;

    Ok(())
}

/// Best-effort peer display name extraction. Falls back to an empty string
/// if the payload is malformed — the session is already Finalized by the
/// time we get here, so we must deliver *some* name to `on_finalized`.
fn contact_name_from_payload(data: &[u8]) -> String {
    deserialize_exchange_payload(data)
        .map(|(_, card)| card.display_name().to_string())
        .unwrap_or_default()
}

/// Sleep for up to `total`, waking every ~25 ms to check for cancellation.
fn responsive_sleep(total: Duration, cancel_flag: &AtomicBool) {
    const CHUNK: Duration = Duration::from_millis(25);
    let mut remaining = total;
    while !remaining.is_zero() {
        if cancel_flag.load(Ordering::Relaxed) {
            return;
        }
        let chunk = remaining.min(CHUNK);
        thread::sleep(chunk);
        remaining = remaining.saturating_sub(chunk);
    }
}
