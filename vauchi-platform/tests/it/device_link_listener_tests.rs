// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for the device-link orchestrator listener
//! (Phase 1.9a — surface-API wave).
//!
//! These tests cover Phase 1 of
//! `_private/docs/problems/2026-04-25-device-link-orchestrator/`:
//! listener registration, cycle-thread lifecycle, cancellation
//! idempotency, listener rebind, and post-cancel action method
//! safety.
//!
//! Wave A (this file) constructs sessions via the harness
//! `MobileDeviceLinkSession::new_initiator_for_test` and points the
//! transport at a dead localhost port (`http://127.0.0.1:1`) so the
//! relay-poll round-trip fails fast. Tests that need to drive the
//! full protocol cycle through a stub relay (cancel-during-relay-wait,
//! qr-expiry deadline, two-session peer bridge, thread-safety
//! proptest) are deferred to Phase 1.9b alongside the Phase 4
//! real-network device gate — both pieces share the relay-stub
//! infrastructure that work will introduce.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

use vauchi_core::exchange::device_link::DeviceLinkQR;
use vauchi_core::identity::Identity;
use vauchi_core::network::{HttpTransport, HttpTransportConfig};
use vauchi_platform::{DeviceLinkSessionListener, MobileDeviceLinkSession};

// ── Mock listener ───────────────────────────────────────────────────

#[derive(Default)]
struct RecordingListener {
    qr_ready: Mutex<Vec<(String, u64)>>,
    confirmation_required: Mutex<Vec<String>>, // device_name only — code/fp not asserted
    request_sent: Mutex<Vec<String>>,
    completed: Mutex<Vec<(String, u32)>>,
    failed: Mutex<Vec<String>>,
    session_ended_count: AtomicU32,
}

impl RecordingListener {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn qr_ready_count(&self) -> usize {
        self.qr_ready.lock().expect("qr_ready").len()
    }

    fn failed_reasons(&self) -> Vec<String> {
        self.failed.lock().expect("failed").clone()
    }

    fn session_ended_count(&self) -> u32 {
        self.session_ended_count.load(Ordering::SeqCst)
    }
}

impl DeviceLinkSessionListener for RecordingListener {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64) {
        self.qr_ready
            .lock()
            .expect("qr_ready")
            .push((qr_data, expires_at_unix));
    }

    fn on_confirmation_required(
        &self,
        device_name: String,
        _confirmation_code: String,
        _identity_fingerprint: String,
        _proximity_challenge: Vec<u8>,
    ) {
        self.confirmation_required
            .lock()
            .expect("confirmation_required")
            .push(device_name);
    }

    fn on_request_sent(&self, code: String) {
        self.request_sent.lock().expect("request_sent").push(code);
    }

    fn on_completed(&self, device_name: String, device_index: u32) {
        self.completed
            .lock()
            .expect("completed")
            .push((device_name, device_index));
    }

    fn on_failed(&self, reason: String) {
        self.failed.lock().expect("failed").push(reason);
    }

    fn on_session_ended(&self) {
        self.session_ended_count.fetch_add(1, Ordering::SeqCst);
    }
}

/// `Box<dyn DeviceLinkSessionListener>` forwarder so the test holds
/// an `Arc` for assertions while the session owns the trait object.
struct ListenerHandle(Arc<RecordingListener>);

impl DeviceLinkSessionListener for ListenerHandle {
    fn on_qr_ready(&self, qr_data: String, expires_at_unix: u64) {
        self.0.on_qr_ready(qr_data, expires_at_unix);
    }
    fn on_confirmation_required(
        &self,
        device_name: String,
        confirmation_code: String,
        identity_fingerprint: String,
        proximity_challenge: Vec<u8>,
    ) {
        self.0.on_confirmation_required(
            device_name,
            confirmation_code,
            identity_fingerprint,
            proximity_challenge,
        );
    }
    fn on_request_sent(&self, code: String) {
        self.0.on_request_sent(code);
    }
    fn on_completed(&self, device_name: String, device_index: u32) {
        self.0.on_completed(device_name, device_index);
    }
    fn on_failed(&self, reason: String) {
        self.0.on_failed(reason);
    }
    fn on_session_ended(&self) {
        self.0.on_session_ended();
    }
}

fn boxed(listener: &Arc<RecordingListener>) -> Box<dyn DeviceLinkSessionListener> {
    Box::new(ListenerHandle(Arc::clone(listener)))
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Construct a session pointed at a dead localhost port. Any cycle
/// thread that reaches `create_offer` will fail fast with a network
/// error, so tests that wait for `on_failed` get a deterministic
/// short timeout.
fn session_against_dead_relay() -> Arc<MobileDeviceLinkSession> {
    let identity = Identity::create("Alice");
    let registry = identity.initial_device_registry();
    let initiator = identity.create_device_link_initiator(registry);
    let identity_id = hex::encode(identity.signing_public_key());

    let config = HttpTransportConfig::for_testing("http://127.0.0.1:1", 500);
    let transport = HttpTransport::new(config);

    Arc::new(MobileDeviceLinkSession::new_initiator_for_test(
        initiator,
        transport,
        identity_id,
        // Generous relay timeout so the create_offer network failure
        // (not the deadline) is what closes Wave-A tests.
        300,
    ))
}

/// Wait up to `total` for `pred` to become true, polling every 25 ms.
fn wait_until<F: Fn() -> bool>(total: Duration, pred: F) -> bool {
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if pred() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    pred()
}

// ── Test 1 — registration + start emits on_qr_ready ────────────────

// @internal
#[test]
fn device_link_listener_registers_and_starts() {
    let session = session_against_dead_relay();
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.start();

    // The cycle thread emits on_qr_ready immediately (no transport
    // round-trip required). Then create_offer fails against the
    // dead port, so on_failed + on_session_ended follow.
    let observed = wait_until(Duration::from_secs(5), || {
        listener.qr_ready_count() >= 1 && listener.session_ended_count() >= 1
    });
    assert!(
        observed,
        "expected at least one on_qr_ready and one on_session_ended; got qr_ready={} session_ended={}",
        listener.qr_ready_count(),
        listener.session_ended_count()
    );

    let qr_payloads = listener.qr_ready.lock().expect("qr_ready");
    assert_eq!(qr_payloads.len(), 1, "on_qr_ready fires exactly once");
    let (qr_data, expires_at) = &qr_payloads[0];
    assert!(!qr_data.is_empty(), "QR data is non-empty");
    let parsed = DeviceLinkQR::from_data_string(qr_data)
        .expect("listener-emitted QR data must round-trip through the QR codec");
    assert_eq!(
        parsed.expires_at(),
        *expires_at,
        "listener-reported expires_at_unix matches the QR's protocol-defined expiry"
    );

    assert_eq!(
        listener.session_ended_count(),
        1,
        "on_session_ended fires exactly once across the entire session lifetime"
    );
}

// ── Test 2 — cancel idempotency before / during / after start ──────

// @internal
#[test]
fn device_link_cancel_idempotent() {
    // Cancel before start: no thread spawned, no callbacks.
    {
        let session = session_against_dead_relay();
        let listener = RecordingListener::new();
        session.set_listener(boxed(&listener));
        session.cancel();
        session.cancel();
        assert!(
            session.cycle_thread_finished_for_test(),
            "thread is finished (never started)"
        );
        assert_eq!(
            listener.session_ended_count(),
            0,
            "no session_ended without a started cycle"
        );
    }

    // Multiple cancels after start: thread joins, exactly one
    // on_session_ended fires.
    {
        let session = session_against_dead_relay();
        let listener = RecordingListener::new();
        session.set_listener(boxed(&listener));
        session.start();

        // Wait for at least one callback to confirm the thread is
        // running, then cancel multiple times.
        wait_until(Duration::from_secs(2), || listener.qr_ready_count() >= 1);

        session.cancel();
        session.cancel();
        session.cancel();

        assert!(
            session.cycle_thread_finished_for_test(),
            "cancel joined the cycle thread"
        );
        // Exactly one on_session_ended regardless of cancel count.
        // The thread observes the cancel flag in run_initiator_cycle
        // (CycleOutcome::Cancelled) and fires session_ended once on
        // exit.
        assert_eq!(
            listener.session_ended_count(),
            1,
            "on_session_ended fires exactly once even for multiple cancels"
        );
    }
}

// ── Test 4 — set_listener replaces the previous registration ──────
//
// The mid-session rebind path (rebind WHILE the cycle thread is in
// the relay-poll loop, then verify the *new* listener gets all
// post-rebind callbacks) needs a stub relay slow enough to give
// the rebind a window — that's Phase 1.9b. Wave A verifies the
// simpler invariant: set_listener replaces the slot, and only the
// most-recently-set listener observes any callbacks fired after
// the replacement.

// @internal
#[test]
fn device_link_listener_set_replaces_previous() {
    let session = session_against_dead_relay();
    let first = RecordingListener::new();
    let second = RecordingListener::new();

    // Register first, immediately replace with second BEFORE start.
    session.set_listener(boxed(&first));
    session.set_listener(boxed(&second));
    session.start();

    // Wait for the cycle to terminate against the dead relay.
    let observed = wait_until(Duration::from_secs(5), || second.session_ended_count() >= 1);
    assert!(
        observed,
        "second listener observes session_ended within the timeout"
    );

    assert_eq!(
        second.session_ended_count(),
        1,
        "second (currently-registered) listener gets session_ended"
    );
    assert_eq!(
        first.session_ended_count(),
        0,
        "first (replaced) listener never receives any callbacks"
    );
    assert_eq!(
        first.qr_ready_count(),
        0,
        "first (replaced) listener never receives any callbacks"
    );
}

// ── Test 7 — user action methods after cancel are no-ops ──────────

// @internal
#[test]
fn device_link_user_action_after_cancel_is_noop() {
    let session = session_against_dead_relay();
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));

    // Cancel before start: action methods must not panic and must
    // not deliver a confirmation to a non-running cycle thread.
    session.cancel();
    let res_manual = session.confirm_manual("123-456".to_string(), 1_700_000_000);
    let res_ultrasonic = session.confirm_ultrasonic(vec![0u8; 16], 1_700_000_000);
    session.deny();
    assert!(
        res_manual.is_ok(),
        "confirm_manual is idempotent post-cancel"
    );
    assert!(
        res_ultrasonic.is_ok(),
        "confirm_ultrasonic is idempotent post-cancel"
    );
    assert_eq!(
        listener.failed_reasons().len(),
        0,
        "no failure callbacks fire from action methods after a pre-start cancel"
    );

    // After start + cancel, action methods stay safe; the bounded
    // channel may already be full from the cancel sentinel, in which
    // case try_send drops silently.
    let session = session_against_dead_relay();
    let listener = RecordingListener::new();
    session.set_listener(boxed(&listener));
    session.start();
    wait_until(Duration::from_secs(2), || listener.qr_ready_count() >= 1);
    session.cancel();

    let res = session.confirm_manual("999".into(), 1_700_000_000);
    let res2 = session.confirm_ultrasonic(vec![1u8; 16], 1_700_000_000);
    session.deny();
    assert!(res.is_ok(), "confirm_manual safe after start+cancel");
    assert!(res2.is_ok(), "confirm_ultrasonic safe after start+cancel");

    // Ultrasonic challenge_response length validation must still
    // fire — that gate is at the action method, not the channel.
    let too_short = session.confirm_ultrasonic(vec![0u8; 15], 1_700_000_000);
    assert!(
        too_short.is_err(),
        "confirm_ultrasonic still rejects wrong-length challenge_response"
    );
}
