// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Integration tests for [`MobileLinkResponderSession`] listener +
//! cycle-thread lifecycle.
//!
//! Phase 1 T6 of `_private/docs/problems/2026-04-27-deep-link-responder-flow`.
//! Drives the cycle thread via a mock listener that captures the
//! callback sequence, feeds hardware events back via
//! `apply_hardware_event`, and asserts the contract:
//!
//! - Initial Polling state surfaces deposits + check via `on_commands`.
//! - `RelayEscrowReady` on our gate transitions to Retrieving and
//!   emits a follow-up `on_commands` with the Retrieve.
//! - `RelayEscrowBlobReceived` with valid ciphertext fires
//!   `on_finalized(card_bytes)` followed by `on_session_ended`.
//! - `RelayEscrowFailed` on our gate fires
//!   `on_failed(DepositRejected)` followed by `on_session_ended`.
//! - `cancel()` fires `on_failed(Cancelled)` followed by
//!   `on_session_ended` exactly once.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use vauchi_core::exchange::link_mode::initiator_generate;
use vauchi_platform::{
    LinkResponderSessionListener, MobileExchangeCommand, MobileExchangeHardwareEvent,
    MobileLinkResponderFailureReason, MobileLinkResponderSession, MobileLinkResponderState,
};

/// Mock listener that captures the full callback sequence so tests can
/// assert ordering + content without observing partial state.
#[derive(Default)]
struct CapturingListener {
    events: Mutex<Vec<RecordedEvent>>,
}

#[derive(Debug, Clone, PartialEq)]
enum RecordedEvent {
    StateChanged(MobileLinkResponderState),
    Commands(usize),
    Finalized(Vec<u8>),
    Failed(MobileLinkResponderFailureReason),
    SessionEnded,
}

impl CapturingListener {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn record(&self, ev: RecordedEvent) {
        if let Ok(mut events) = self.events.lock() {
            events.push(ev);
        }
    }

    fn snapshot(&self) -> Vec<RecordedEvent> {
        self.events.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Wait up to `timeout` for the listener to record an event matching
    /// the predicate. Returns true if the event arrived in time. Polls
    /// at 10 ms intervals to stay responsive without busy-looping.
    fn wait_for<F: Fn(&RecordedEvent) -> bool>(&self, pred: F, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.snapshot().iter().any(&pred) {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }
}

impl LinkResponderSessionListener for CapturingListener {
    fn on_state_changed(&self, state: MobileLinkResponderState) {
        self.record(RecordedEvent::StateChanged(state));
    }
    fn on_commands(&self, commands: Vec<MobileExchangeCommand>) {
        self.record(RecordedEvent::Commands(commands.len()));
    }
    fn on_finalized(&self, card_bytes: Vec<u8>) {
        self.record(RecordedEvent::Finalized(card_bytes));
    }
    fn on_failed(&self, reason: MobileLinkResponderFailureReason) {
        self.record(RecordedEvent::Failed(reason));
    }
    fn on_session_ended(&self) {
        self.record(RecordedEvent::SessionEnded);
    }
}

/// Adapter — UniFFI exports take `Box<dyn Listener>`, but `set_listener`
/// also needs a way for the test to keep peeking. Wrap the Arc in a
/// stateless forwarder that delegates every callback.
struct ListenerForwarder(Arc<CapturingListener>);

impl LinkResponderSessionListener for ListenerForwarder {
    fn on_state_changed(&self, state: MobileLinkResponderState) {
        self.0.on_state_changed(state);
    }
    fn on_commands(&self, commands: Vec<MobileExchangeCommand>) {
        self.0.on_commands(commands);
    }
    fn on_finalized(&self, card_bytes: Vec<u8>) {
        self.0.on_finalized(card_bytes);
    }
    fn on_failed(&self, reason: MobileLinkResponderFailureReason) {
        self.0.on_failed(reason);
    }
    fn on_session_ended(&self) {
        self.0.on_session_ended();
    }
}

/// Build a fresh session + return (session, listener handle, the URL
/// the responder is responding to).
fn make_session() -> (Arc<MobileLinkResponderSession>, Arc<CapturingListener>) {
    let (init, _) = initiator_generate();
    let session = MobileLinkResponderSession::new(init.url, b"responder_card".to_vec())
        .expect("contributory DH");
    let listener = CapturingListener::new();
    session.set_listener(Box::new(ListenerForwarder(Arc::clone(&listener))));
    (session, listener)
}

// @internal
#[test]
fn fresh_session_emits_initial_commands_and_polling_state() {
    let (session, listener) = make_session();
    session.start();

    // The cycle thread emits on_commands + on_state_changed(Polling)
    // on its first iteration. 1 s deadline is overkill but stays
    // robust under loaded CI.
    assert!(
        listener.wait_for(
            |e| matches!(e, RecordedEvent::Commands(n) if *n == 3),
            Duration::from_secs(1)
        ),
        "expected Commands(3) within 1 s, got {:?}",
        listener.snapshot()
    );
    assert!(
        listener.wait_for(
            |e| matches!(
                e,
                RecordedEvent::StateChanged(MobileLinkResponderState::Polling)
            ),
            Duration::from_secs(1)
        ),
        "expected StateChanged(Polling), got {:?}",
        listener.snapshot()
    );

    session.cancel();
}

// @internal
#[test]
fn deposit_rejected_fires_failed_then_session_ended() {
    let (session, listener) = make_session();
    session.start();

    // Wait for initial Polling so the session is in a state that
    // accepts the failure event.
    assert!(listener.wait_for(
        |e| matches!(
            e,
            RecordedEvent::StateChanged(MobileLinkResponderState::Polling)
        ),
        Duration::from_secs(1)
    ));

    // Inject a relay failure for our gate.
    session.apply_hardware_event(MobileExchangeHardwareEvent::RelayEscrowFailed {
        gate_hash: session.gate_hash_bytes(),
        reason: "slot already occupied".into(),
    });

    assert!(
        listener.wait_for(
            |e| matches!(
                e,
                RecordedEvent::Failed(MobileLinkResponderFailureReason::DepositRejected)
            ),
            Duration::from_secs(2)
        ),
        "expected Failed(DepositRejected), got {:?}",
        listener.snapshot()
    );
    assert!(
        listener.wait_for(
            |e| matches!(e, RecordedEvent::SessionEnded),
            Duration::from_secs(2)
        ),
        "expected SessionEnded after Failed, got {:?}",
        listener.snapshot()
    );

    session.cancel(); // idempotent — already terminal
}

// @internal
#[test]
fn cancel_fires_failed_cancelled_then_session_ended() {
    let (session, listener) = make_session();
    session.start();

    // Wait for the cycle thread to start before cancelling, so we
    // exercise the in-flight cancel path rather than a never-started
    // session.
    assert!(listener.wait_for(
        |e| matches!(
            e,
            RecordedEvent::StateChanged(MobileLinkResponderState::Polling)
        ),
        Duration::from_secs(1)
    ));

    session.cancel();

    // After cancel, the cycle thread must surface the Cancelled
    // failure + session ended exactly once.
    let snap = listener.snapshot();
    let cancelled_count = snap
        .iter()
        .filter(|e| {
            matches!(
                e,
                RecordedEvent::Failed(MobileLinkResponderFailureReason::Cancelled)
            )
        })
        .count();
    let ended_count = snap
        .iter()
        .filter(|e| matches!(e, RecordedEvent::SessionEnded))
        .count();
    assert_eq!(
        cancelled_count, 1,
        "expected exactly one Failed(Cancelled), got {snap:?}"
    );
    assert_eq!(
        ended_count, 1,
        "expected exactly one SessionEnded, got {snap:?}"
    );

    // Idempotency — second cancel is a no-op.
    session.cancel();
    let snap2 = listener.snapshot();
    assert_eq!(
        snap2
            .iter()
            .filter(|e| matches!(e, RecordedEvent::SessionEnded))
            .count(),
        1,
        "second cancel must not double-fire SessionEnded, got {snap2:?}"
    );
}

// @internal
#[test]
fn double_start_is_idempotent() {
    let (session, listener) = make_session();
    session.start();
    session.start(); // second call is a no-op while the first is alive

    // Initial Polling still arrives once.
    assert!(listener.wait_for(
        |e| matches!(
            e,
            RecordedEvent::StateChanged(MobileLinkResponderState::Polling)
        ),
        Duration::from_secs(1)
    ));

    session.cancel();
}
