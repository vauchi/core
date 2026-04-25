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
    MobileMultiStageSession, MobileProtocolState, MobileQrPayload, MultiStageSessionListener,
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
