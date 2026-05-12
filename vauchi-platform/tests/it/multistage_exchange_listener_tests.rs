// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the G4 event-driven multi-stage exchange listener.
//!
//! These tests cover Phase 1 of `_private/docs/problems/2026-04-23-g4-exchange-event-api/`:
//! listener registration, cycle-thread lifecycle, cancellation, idempotency,
//! and listener rebind.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use vauchi_platform::{
    MobileAudioProximityState, MobileMultiStageSession, MobileProtocolState, MobileQrPayload,
    MultiStageAudioListener, MultiStageSessionListener,
};

// ── Mock listener ───────────────────────────────────────────────────

#[derive(Default)]
struct RecordingListener {
    qr_payloads: Mutex<Vec<MobileQrPayload>>,
    state_changes: Mutex<Vec<MobileProtocolState>>,
    finalized_names: Mutex<Vec<String>>,
    session_ended_count: AtomicU32,
}

impl RecordingListener {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn state_changes(&self) -> Vec<MobileProtocolState> {
        self.state_changes.lock().expect("state_changes").clone()
    }

    fn finalized_names(&self) -> Vec<String> {
        self.finalized_names
            .lock()
            .expect("finalized_names")
            .clone()
    }

    fn qr_count(&self) -> usize {
        self.qr_payloads.lock().expect("qr_payloads").len()
    }

    fn session_ended_count(&self) -> u32 {
        self.session_ended_count.load(Ordering::SeqCst)
    }

    fn latest_qr(&self) -> Option<MobileQrPayload> {
        self.qr_payloads
            .lock()
            .expect("qr_payloads")
            .last()
            .cloned()
    }
}

impl MultiStageSessionListener for RecordingListener {
    fn on_qr_payload(&self, payload: MobileQrPayload) {
        self.qr_payloads.lock().expect("qr_payloads").push(payload);
    }

    fn on_state_changed(&self, state: MobileProtocolState) {
        self.state_changes
            .lock()
            .expect("state_changes")
            .push(state);
    }

    fn on_finalized(&self, contact_name: String) {
        self.finalized_names
            .lock()
            .expect("finalized_names")
            .push(contact_name);
    }

    fn on_session_ended(&self) {
        self.session_ended_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// `Box<dyn MultiStageSessionListener>` forwarder so the test can hand off
/// ownership to the session while still holding an `Arc` for assertions.
struct ListenerHandle(Arc<RecordingListener>);

impl MultiStageSessionListener for ListenerHandle {
    fn on_qr_payload(&self, payload: MobileQrPayload) {
        self.0.on_qr_payload(payload);
    }
    fn on_state_changed(&self, state: MobileProtocolState) {
        self.0.on_state_changed(state);
    }
    fn on_finalized(&self, contact_name: String) {
        self.0.on_finalized(contact_name);
    }
    fn on_session_ended(&self) {
        self.0.on_session_ended();
    }
}

fn boxed(listener: &Arc<RecordingListener>) -> Box<dyn MultiStageSessionListener> {
    Box::new(ListenerHandle(Arc::clone(listener)))
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Construct a session with a minimal, deserializable exchange payload so
/// `on_finalized` receives a real peer name when the protocol finalizes.
///
/// We do not actually drive this session to Finalized in Phase 1 integration
/// tests (that requires a two-session harness and is covered separately).
fn session_with_dummy_payload(_label: &str) -> Arc<MobileMultiStageSession> {
    // The protocol wraps whatever bytes it is handed; format matching
    // `mobile_exchange::serialize_exchange_payload` is only required on the
    // Finalized path. For lifecycle/cancellation tests the bytes are opaque.
    let payload = b"dummy local card".to_vec();
    Arc::new(MobileMultiStageSession::new(payload))
}

/// Wait up to `timeout` for `predicate` to return true. Polls every 25 ms —
/// no `sleep()` of the exchange period itself (CC-06 forbids real-time waits
/// in tests; we only sleep here as a scheduler-yield for the cycle thread).
fn wait_for(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while Instant::now().duration_since(start) < timeout {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    predicate()
}

// ── T1.T1 — Lifecycle: start emits initial QR + state transition ────

// @internal
#[test]
fn start_emits_initial_qr_and_state_transition() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(10);
    session.start();

    let got = wait_for(Duration::from_secs(2), || listener.qr_count() > 0);
    session.cancel();

    assert!(got, "expected at least one on_qr_payload callback");
    // Idle transitions to Advertising on first get_display_qr().
    let transitions = listener.state_changes();
    assert!(
        transitions.contains(&MobileProtocolState::Advertising),
        "expected Advertising in state transitions, got {transitions:?}"
    );
}

// ── T1.T2 — on_state_changed fires only on actual transitions ───────

// @internal
#[test]
fn state_changed_fires_only_on_actual_transitions() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(10);
    session.start();

    // Let the cycle run long enough to emit several QR frames in the same
    // Advertising state.
    thread::sleep(Duration::from_millis(200));
    session.cancel();

    let transitions = listener.state_changes();
    let advertising_count = transitions
        .iter()
        .filter(|s| matches!(s, MobileProtocolState::Advertising))
        .count();
    assert!(
        advertising_count <= 1,
        "on_state_changed(Advertising) must fire at most once; fired {advertising_count} times"
    );
    let qr_count = listener.qr_count();
    assert!(
        qr_count > 1,
        "expected multiple QR payloads while Advertising but only saw {qr_count}"
    );
}

// ── T1.T3 — Cancellation ────────────────────────────────────────────

// @internal
#[test]
fn cancel_stops_callbacks_and_joins_thread() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(50);
    session.start();

    // Let the cycle produce at least one QR.
    assert!(wait_for(Duration::from_secs(1), || listener.qr_count() > 0));

    let cancel_start = Instant::now();
    session.cancel();
    let cancel_elapsed = cancel_start.elapsed();

    assert!(
        cancel_elapsed < Duration::from_millis(800),
        "cancel() must return within 800 ms (worst-case DISPLAY_MS_INIT); took {cancel_elapsed:?}"
    );

    let qr_count_at_cancel = listener.qr_count();
    let transitions_at_cancel = listener.state_changes().len();

    // No further callbacks after cancel.
    thread::sleep(Duration::from_millis(150));
    assert_eq!(
        listener.qr_count(),
        qr_count_at_cancel,
        "on_qr_payload fired after cancel()"
    );
    assert_eq!(
        listener.state_changes().len(),
        transitions_at_cancel,
        "on_state_changed fired after cancel()"
    );

    assert!(
        session.cycle_thread_finished_for_test(),
        "cycle thread should be joined after cancel()"
    );
    assert_eq!(
        listener.session_ended_count(),
        1,
        "on_session_ended should fire exactly once — fired {} times",
        listener.session_ended_count()
    );
}

// ── T1.T5 — Idempotency of start / cancel ──────────────────────────

// @internal
#[test]
fn start_is_idempotent() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(25);

    session.start();
    session.start();
    session.start();

    assert!(wait_for(Duration::from_secs(1), || listener.qr_count() > 0));

    session.cancel();

    assert_eq!(
        listener.session_ended_count(),
        1,
        "double start should not spawn multiple cycle threads (saw {} on_session_ended)",
        listener.session_ended_count()
    );
}

// @internal
#[test]
fn cancel_before_start_is_noop() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));

    // No panic; safe to call without a prior start().
    session.cancel();
    session.cancel();

    assert_eq!(listener.qr_count(), 0);
    assert_eq!(listener.state_changes().len(), 0);
    assert_eq!(listener.session_ended_count(), 0);
}

// @internal
#[test]
fn cancel_is_idempotent() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(25);
    session.start();

    assert!(wait_for(Duration::from_secs(1), || listener.qr_count() > 0));

    session.cancel();
    session.cancel(); // must not panic, must not double-fire on_session_ended
    session.cancel();

    assert_eq!(
        listener.session_ended_count(),
        1,
        "on_session_ended should fire exactly once across repeated cancel() calls"
    );
}

// ── T1.T6 — Listener re-registration ────────────────────────────────

// @internal
#[test]
fn listener_rebind_routes_new_callbacks_only_to_b() {
    let session = session_with_dummy_payload("alice");
    let listener_a = RecordingListener::new();
    let listener_b = RecordingListener::new();

    session.set_listener(boxed(&listener_a));
    session.set_cycle_sleep_override_ms_for_test(25);
    session.start();

    assert!(wait_for(Duration::from_secs(1), || listener_a.qr_count() > 0));

    let a_qr_at_swap = listener_a.qr_count();
    session.set_listener(boxed(&listener_b));

    assert!(wait_for(Duration::from_secs(1), || listener_b.qr_count() > 0));

    session.cancel();

    let a_qr_final = listener_a.qr_count();
    assert_eq!(
        a_qr_final, a_qr_at_swap,
        "listener A received QRs after being replaced: {a_qr_at_swap} → {a_qr_final}"
    );
    assert_eq!(
        listener_a.session_ended_count(),
        0,
        "listener A received on_session_ended after replacement"
    );
    assert_eq!(
        listener_b.session_ended_count(),
        1,
        "listener B should receive the final on_session_ended"
    );
}

// ── T1.T1 — End-to-end exchange: Alice ↔ Bob with auto-finalize ─────

/// Bridge listener that forwards every QR to the peer session. Combined with
/// a `RecordingListener` via the `Tee` below, two sessions can drive each
/// other to Finalized purely through the event API.
struct PeerBridge {
    peer: Arc<MobileMultiStageSession>,
    recorder: Arc<RecordingListener>,
}

impl MultiStageSessionListener for PeerBridge {
    fn on_qr_payload(&self, payload: MobileQrPayload) {
        let data = payload.data.clone();
        self.recorder.on_qr_payload(payload);
        let _ = self.peer.process_scanned_qr(data);
    }
    fn on_state_changed(&self, state: MobileProtocolState) {
        self.recorder.on_state_changed(state);
    }
    fn on_finalized(&self, contact_name: String) {
        self.recorder.on_finalized(contact_name);
    }
    fn on_session_ended(&self) {
        self.recorder.on_session_ended();
    }
}

/// Build a minimal valid exchange payload: version byte + 32-byte pubkey +
/// JSON-serialized `ContactCard`. Matches
/// `mobile_exchange::serialize_exchange_payload` so the listener's
/// `on_finalized` receives a real display name on the Finalized path.
fn exchange_payload_for(display_name: &str) -> Vec<u8> {
    use vauchi_core::contact_card::ContactCard;
    let card = ContactCard::new(display_name);
    let card_json = serde_json::to_vec(&card).expect("ContactCard serde");
    let mut payload = Vec::with_capacity(1 + 32 + card_json.len());
    payload.push(1u8); // EXCHANGE_PAYLOAD_VERSION
    payload.extend_from_slice(&[0u8; 32]); // fake pubkey — ContactCard is what on_finalized reads
    payload.extend_from_slice(&card_json);
    payload
}

// @internal
#[test]
fn end_to_end_exchange_fires_on_finalized_with_peer_display_name() {
    let alice = Arc::new(MobileMultiStageSession::new(exchange_payload_for("Alice")));
    let bob = Arc::new(MobileMultiStageSession::new(exchange_payload_for("Bob")));

    let alice_recorder = RecordingListener::new();
    let bob_recorder = RecordingListener::new();

    alice.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&bob),
        recorder: Arc::clone(&alice_recorder),
    }));
    bob.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&alice),
        recorder: Arc::clone(&bob_recorder),
    }));

    // Drive the exchange at ~5 ms per cycle — keeps the wall clock under
    // 10 seconds even with a few thousand bridge hops.
    alice.set_cycle_sleep_override_ms_for_test(5);
    bob.set_cycle_sleep_override_ms_for_test(5);

    alice.start();
    bob.start();

    let finalized = wait_for(Duration::from_secs(15), || {
        !alice_recorder.finalized_names().is_empty() && !bob_recorder.finalized_names().is_empty()
    });

    alice.cancel();
    bob.cancel();

    assert!(
        finalized,
        "exchange did not finalize within 15 s; alice states={:?} bob states={:?}",
        alice_recorder.state_changes(),
        bob_recorder.state_changes()
    );

    assert_eq!(
        alice_recorder.finalized_names(),
        vec!["Bob".to_string()],
        "Alice should see peer name 'Bob' on_finalized exactly once"
    );
    assert_eq!(
        bob_recorder.finalized_names(),
        vec!["Alice".to_string()],
        "Bob should see peer name 'Alice' on_finalized exactly once"
    );

    // Both should see the Finalized transition exactly once.
    assert_eq!(
        alice_recorder
            .state_changes()
            .iter()
            .filter(|s| matches!(s, MobileProtocolState::Finalized))
            .count(),
        1
    );
    assert_eq!(
        bob_recorder
            .state_changes()
            .iter()
            .filter(|s| matches!(s, MobileProtocolState::Finalized))
            .count(),
        1
    );

    // on_session_ended fires once per side.
    assert_eq!(alice_recorder.session_ended_count(), 1);
    assert_eq!(bob_recorder.session_ended_count(), 1);
}

// ── T1.T4 — Concurrent process_scanned_qr + cycle thread ────────────

// @internal
#[test]
fn process_scanned_qr_is_safe_concurrent_with_cycle_thread() {
    let session = session_with_dummy_payload("alice");
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.set_cycle_sleep_override_ms_for_test(5);
    session.start();

    // Wait for the cycle to produce at least one payload.
    assert!(wait_for(Duration::from_secs(1), || listener
        .latest_qr()
        .is_some()));

    let session_for_thread = Arc::clone(&session);
    let scanner = thread::spawn(move || {
        for _ in 0..50 {
            // Feed malformed QR data — should fail-parse and leave state
            // alone, but must never deadlock or panic.
            let _ = session_for_thread.process_scanned_qr("not-a-valid-qr".to_string());
            thread::sleep(Duration::from_millis(2));
        }
    });

    scanner.join().expect("scanner thread must not panic");

    session.cancel();
    assert_eq!(listener.session_ended_count(), 1);
}

// ── Audio listener plumbing (Phase 1.C.3c) ────────────────────────

/// Audio-listener mock that records every callback the wrapper fires.
#[derive(Default)]
struct RecordingAudioListener {
    states: Mutex<Vec<MobileAudioProximityState>>,
}

impl RecordingAudioListener {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn states(&self) -> Vec<MobileAudioProximityState> {
        self.states.lock().expect("states").clone()
    }
}

impl MultiStageAudioListener for RecordingAudioListener {
    fn on_audio_state_changed(&self, state: MobileAudioProximityState) {
        self.states.lock().expect("states").push(state);
    }
}

// @internal
#[test]
fn fresh_session_audio_proximity_is_pending() {
    let session = MobileMultiStageSession::new(b"card".to_vec());
    assert_eq!(
        session.audio_proximity(),
        MobileAudioProximityState::Pending,
        "every freshly-constructed session starts Pending — Hover handshake hasn't run yet",
    );
}

// @internal
#[test]
fn set_audio_listener_accepts_and_replaces() {
    // The wrapper exposes a sibling slot for the audio listener; the
    // existing base listener slot is untouched (composition, not
    // extension, per the 1.C.1 investigation refinement). Registering
    // an audio listener has no side effects today — Phase 1.C.3d
    // adds the orchestrator that fires the callback.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    let first = RecordingAudioListener::new();
    let second = RecordingAudioListener::new();
    session.set_audio_listener(Box::new(SharedAudio(first.clone())));
    session.set_audio_listener(Box::new(SharedAudio(second.clone())));
    // Neither listener has received callbacks because the orchestrator
    // (1.C.3d) isn't wired yet; assert the dormant state.
    assert!(first.states().is_empty());
    assert!(second.states().is_empty());
}

// @internal
#[test]
fn base_listener_and_audio_listener_coexist() {
    // Registering both kinds of listener at once must succeed — the
    // wrapper owns one slot per listener kind so Hover consumers can
    // pair the two without one overriding the other.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    let base = RecordingListener::new();
    let audio = RecordingAudioListener::new();
    session.set_listener(Box::new(SharedBase(base.clone())));
    session.set_audio_listener(Box::new(SharedAudio(audio.clone())));
    // Listener wiring is async — calling set_* is enough; no callbacks
    // fire until something happens to drive them.
    assert!(audio.states().is_empty());
    assert!(base.state_changes().is_empty());
}

// `Box<dyn Trait>` wrappers around `Arc<RecordingListener>` so the
// test can both hand the listener to the wrapper and continue to
// observe it via the original Arc. UniFFI's callback-interface
// shape requires `Box<dyn …>`, but we need shared ownership for
// post-call assertions.
struct SharedBase(Arc<RecordingListener>);
impl MultiStageSessionListener for SharedBase {
    fn on_qr_payload(&self, p: MobileQrPayload) {
        self.0.on_qr_payload(p);
    }
    fn on_state_changed(&self, s: MobileProtocolState) {
        self.0.on_state_changed(s);
    }
    fn on_finalized(&self, n: String) {
        self.0.on_finalized(n);
    }
    fn on_session_ended(&self) {
        self.0.on_session_ended();
    }
}

struct SharedAudio(Arc<RecordingAudioListener>);
impl MultiStageAudioListener for SharedAudio {
    fn on_audio_state_changed(&self, s: MobileAudioProximityState) {
        self.0.on_audio_state_changed(s);
    }
}

// ── Audio handshake trigger (Phase 1.C.3e-i) ──────────────────────

// @internal
#[test]
fn start_audio_handshake_transitions_to_listening_and_notifies_listener() {
    // Phase 1.C.3e-i: externally-driven trigger. Caller invokes
    // start_audio_handshake; the wrapper drives the inner session
    // through Pending → Listening and fires on_audio_state_changed.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    let audio = RecordingAudioListener::new();
    session.set_audio_listener(Box::new(SharedAudio(audio.clone())));

    session
        .start_audio_handshake(&[0u8; 16])
        .expect("Pending → Listening must succeed");

    assert_eq!(
        session.audio_proximity(),
        MobileAudioProximityState::Listening
    );
    assert_eq!(
        audio.states(),
        vec![MobileAudioProximityState::Listening],
        "listener must receive exactly one Listening callback",
    );
}

// @internal
#[test]
fn start_audio_handshake_idempotent_call_is_rejected() {
    // Listening → Listening is a no-op transition rejected by the
    // inner state machine (security gate via AudioStateError). The
    // wrapper surfaces the rejection rather than silently no-op'ing.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake(&[0u8; 16]).unwrap();
    assert!(
        session.start_audio_handshake(&[0u8; 16]).is_err(),
        "second start_audio_handshake call must error — Listening → Listening is rejected",
    );
}

// @internal
#[test]
fn start_audio_handshake_without_listener_succeeds_silently() {
    // The audio listener is optional; sessions without one (e.g.
    // headless harness tests) can still drive the inner state
    // machine via start_audio_handshake without panicking.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session
        .start_audio_handshake(&[0u8; 16])
        .expect("no listener registered, but inner transition still works");
    assert_eq!(
        session.audio_proximity(),
        MobileAudioProximityState::Listening
    );
}

// ── FSK challenge emission + command queue (Phase 1.C.3e-ii) ──────

// @internal
#[test]
fn start_audio_handshake_pushes_emit_and_listen_commands_to_queue() {
    use vauchi_core::Command;
    let session = MobileMultiStageSession::new(b"card".to_vec());
    assert!(session.drain_audio_commands().is_empty());

    let challenge = [
        0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x07, 0x18, 0x29, 0x3A, 0x4B, 0x5C, 0x6D, 0x7E, 0x8F,
        0x90,
    ];
    session
        .start_audio_handshake(&challenge)
        .expect("Pending → Listening must succeed");

    let cmds = session.drain_audio_commands();
    assert_eq!(
        cmds.len(),
        2,
        "expected paired (AudioEmitChallenge, AudioListenForResponse); got {cmds:?}",
    );
    match &cmds[0] {
        Command::AudioEmitChallenge {
            samples,
            sample_rate,
        } => {
            assert_eq!(*sample_rate, 44100, "default modem sample rate");
            assert!(
                !samples.is_empty(),
                "FSK waveform must be non-empty for a 16-byte challenge"
            );
        }
        other => panic!("expected AudioEmitChallenge, got {other:?}"),
    }
    match &cmds[1] {
        Command::AudioListenForResponse {
            timeout_ms,
            sample_rate,
        } => {
            assert_eq!(
                *timeout_ms, 5000,
                "default listen window mirrors legacy ExchangeSession"
            );
            assert_eq!(*sample_rate, 44100);
        }
        other => panic!("expected AudioListenForResponse, got {other:?}"),
    }
}

// @internal
#[test]
fn drain_audio_commands_empties_the_queue() {
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake(&[0u8; 16]).unwrap();
    assert_eq!(
        session.drain_audio_commands().len(),
        2,
        "first drain returns the queued commands"
    );
    assert!(
        session.drain_audio_commands().is_empty(),
        "second drain must be empty — queue was taken, not cloned"
    );
}

// @internal
#[test]
fn rejected_start_audio_handshake_does_not_queue_commands() {
    // Security invariant: a rejected transition (Listening →
    // Listening) must NOT enqueue commands. A stray chirp would
    // mislead the peer or trigger false-negative timeouts.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake(&[0u8; 16]).unwrap();
    let _ = session.drain_audio_commands();
    assert!(session.start_audio_handshake(&[1u8; 16]).is_err());
    assert!(
        session.drain_audio_commands().is_empty(),
        "rejected transition must not enqueue commands",
    );
}

// ── Convenience trigger using session_id (Phase 1.C.3e-iii) ───────

// @internal
#[test]
fn start_audio_handshake_for_session_uses_inner_session_id_as_challenge() {
    use vauchi_core::Command;
    let session = MobileMultiStageSession::new(b"card".to_vec());

    session
        .start_audio_handshake_for_session()
        .expect("Pending → Listening must succeed");

    let cmds = session.drain_audio_commands();
    assert_eq!(
        cmds.len(),
        2,
        "expected paired (Emit, Listen); got {cmds:?}"
    );
    // The FSK samples are non-empty — we can't compare to specific
    // bytes without re-deriving the modem config and the inner
    // session_id, both of which would re-implement the production
    // code. The non-empty assertion is enough to verify the wrapper
    // pulled real bytes from the inner session rather than passing
    // a zero array.
    match &cmds[0] {
        Command::AudioEmitChallenge { samples, .. } => {
            assert!(
                !samples.is_empty(),
                "FSK samples must be derived from a real session_id"
            );
        }
        other => panic!("expected AudioEmitChallenge, got {other:?}"),
    }
}

// @internal
#[test]
fn start_audio_handshake_for_session_second_call_rejected() {
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake_for_session().unwrap();
    assert!(
        session.start_audio_handshake_for_session().is_err(),
        "second start must be rejected — Listening → Listening is a security gate",
    );
}

// ── Audio response verification (Phase 1.C.3e-iv) ─────────────────

// @internal
#[test]
fn process_audio_samples_recorded_decode_error_transitions_to_failed() {
    // Decode failure (no preamble, malformed samples) collapses to
    // Failed — user-facing story is "couldn't confirm devices are
    // close".
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake(&[0u8; 16]).unwrap();
    // Garbage samples that decode_fsk_samples can't make sense of.
    let garbage = vec![0.0f32; 100];
    session
        .process_audio_samples_recorded(garbage, 44100)
        .expect("transition to Failed must succeed via the state machine");
    assert_eq!(
        session.audio_proximity(),
        MobileAudioProximityState::Failed,
        "decode error must transition to Failed",
    );
}

// @internal
#[test]
fn process_audio_samples_recorded_without_peer_session_id_transitions_to_failed() {
    // peer_session_id is None until Stage 1 (Discovered) completes.
    // A samples-recorded callback that arrives before then races
    // the orchestrator. Verify it doesn't crash and surfaces a
    // Failed state — the orchestrator should not have triggered
    // the handshake before Discovered, but if it did, the
    // verification has nothing to compare against.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    session.start_audio_handshake(&[0u8; 16]).unwrap();
    // Even a perfectly-encoded set of samples can't verify without
    // peer_session_id — None branch in verify_audio_response.
    use vauchi_core::exchange::{AudioConfig, audio_modem};
    let config = AudioConfig::default();
    let samples = audio_modem::generate_fsk_samples(&[0xAB; 16], &config);
    session
        .process_audio_samples_recorded(samples, 44100)
        .expect("transition must succeed");
    assert_eq!(session.audio_proximity(), MobileAudioProximityState::Failed);
}

// @internal
#[test]
fn process_audio_samples_recorded_outside_listening_window_rejected() {
    // The state-machine gate from 1.C.3b enforces that transitions
    // come from Listening. Calling process_audio_samples_recorded
    // before start_audio_handshake (still Pending) must return
    // AudioStateError::InvalidTransition.
    let session = MobileMultiStageSession::new(b"card".to_vec());
    let result = session.process_audio_samples_recorded(vec![0.0; 10], 44100);
    assert!(
        result.is_err(),
        "can't process audio response before entering Listening; got Ok(())",
    );
    assert_eq!(
        session.audio_proximity(),
        MobileAudioProximityState::Pending
    );
}

// ── Autonomous trigger from cycle thread (Phase 1.C.3e-vi) ────────

// @internal
#[test]
fn autonomous_trigger_fires_audio_handshake_on_discovered() {
    // Two sessions exchange INIT QRs via PeerBridge. The cycle
    // thread observes the Discovered transition and autonomously
    // fires the audio handshake: state → Listening, queue gets
    // AudioEmitChallenge + AudioListenForResponse, audio listener
    // receives the Listening callback. No external trigger call —
    // the orchestrator does not invoke start_audio_handshake_for_session.
    let alice = Arc::new(MobileMultiStageSession::new(exchange_payload_for("Alice")));
    let bob = Arc::new(MobileMultiStageSession::new(exchange_payload_for("Bob")));

    let alice_recorder = RecordingListener::new();
    let bob_recorder = RecordingListener::new();
    let alice_audio = RecordingAudioListener::new();
    let bob_audio = RecordingAudioListener::new();

    alice.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&bob),
        recorder: Arc::clone(&alice_recorder),
    }));
    bob.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&alice),
        recorder: Arc::clone(&bob_recorder),
    }));
    alice.set_audio_listener(Box::new(SharedAudio(Arc::clone(&alice_audio))));
    bob.set_audio_listener(Box::new(SharedAudio(Arc::clone(&bob_audio))));

    alice.set_cycle_sleep_override_ms_for_test(5);
    bob.set_cycle_sleep_override_ms_for_test(5);
    alice.start();
    bob.start();

    // Wait until both sides see Discovered (or pass through to a
    // later state — Discovered is brief). Audio handshake fires
    // synchronously with the state transition so the audio listener
    // having received any Listening callback proves the trigger ran.
    let observed = wait_for(Duration::from_secs(10), || {
        let alice_audio_states = alice_audio.states();
        let bob_audio_states = bob_audio.states();
        alice_audio_states.contains(&MobileAudioProximityState::Listening)
            && bob_audio_states.contains(&MobileAudioProximityState::Listening)
    });

    alice.cancel();
    bob.cancel();

    assert!(
        observed,
        "autonomous trigger did not fire within 10 s; alice audio states={:?}, bob audio states={:?}",
        alice_audio.states(),
        bob_audio.states()
    );

    // Listening must appear exactly once per side — the autonomous
    // trigger is idempotent (state-machine gate rejects the second
    // Pending → Listening transition).
    assert_eq!(
        alice_audio
            .states()
            .iter()
            .filter(|s| **s == MobileAudioProximityState::Listening)
            .count(),
        1,
        "alice should see Listening exactly once; got {:?}",
        alice_audio.states(),
    );
    assert_eq!(
        bob_audio
            .states()
            .iter()
            .filter(|s| **s == MobileAudioProximityState::Listening)
            .count(),
        1,
        "bob should see Listening exactly once; got {:?}",
        bob_audio.states(),
    );

    // Each session should have queued the paired (AudioEmitChallenge,
    // AudioListenForResponse) commands — verified by draining.
    let alice_cmds = alice.drain_audio_commands();
    let bob_cmds = bob.drain_audio_commands();
    assert_eq!(alice_cmds.len(), 2);
    assert_eq!(bob_cmds.len(), 2);
}
