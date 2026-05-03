// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Phase 2.5 regression test for the G4 listener-path persistence gap.
//!
//! After Phase 2 of the G4 exchange event API
//! (`_private/docs/problems/2026-04-23-g4-exchange-event-api/`),
//! contact persistence on the listener path is broken: a successful
//! face-to-face multi-stage exchange between two `VauchiPlatform`
//! instances drives both sessions to `Finalized` and fires
//! `on_finalized(contact_name)` on each side, but neither peer ends
//! up in the other's storage. The cycle thread inside
//! `MobileMultiStageSession` has no `VauchiPlatform` / `MobileStorage`
//! handle, and the previously-load-bearing
//! `VauchiPlatform::finalize_multistage_exchange` was removed in
//! Phase 3 partial without a replacement.
//!
//! This test asserts the contract Phase 2.5 must restore: after the
//! listener fires `on_finalized`, each peer appears in the other's
//! `list_contacts()` exactly once, and a double-ratchet entry exists
//! for the new contact id. **It is RED on current main and stays RED
//! until Phase 2.5 ships** — gated behind `#[ignore]` so CI does not
//! treat the regression as a pipeline failure but the test inventory
//! still surfaces the gap. Remove the `#[ignore]` as part of the
//! Phase 2.5 fix.
//!
//! Driving the exchange end-to-end requires a two-session
//! `PeerBridge` driver (Alice's QR feeds Bob's `process_scanned_qr`
//! and vice versa) plus the cycle-sleep override that the existing
//! `multistage_exchange_listener_tests::end_to_end_exchange_fires_on_finalized_with_peer_display_name`
//! uses. We diverge from that test only in two places: each side
//! creates its session via a `VauchiPlatform` (so persistence has a
//! place to land), and after `on_finalized` we assert the contact
//! database state.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

use vauchi_platform::{
    MobileMultiStageSession, MobileProtocolState, MobileQrPayload, MultiStageSessionListener,
    VauchiPlatform,
};

/// Recording listener used by both Alice and Bob. Captures the peer
/// display name reported by `on_finalized` and signals when the
/// session ends so the test driver knows when to assert.
#[derive(Default)]
struct RecordingListener {
    finalized_names: Mutex<Vec<String>>,
    session_ended: Mutex<bool>,
}

impl RecordingListener {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn finalized_name(&self) -> Option<String> {
        self.finalized_names
            .lock()
            .expect("finalized_names")
            .first()
            .cloned()
    }

    fn session_ended(&self) -> bool {
        *self.session_ended.lock().expect("session_ended")
    }
}

/// PeerBridge — pumps each side's QR payloads into the other side's
/// `process_scanned_qr` so the two sessions drive each other to
/// `Finalized` without a real camera. Mirrors the harness in
/// `multistage_exchange_listener_tests::PeerBridge` but separated so
/// the listener tests stay focused on Phase 1 callback semantics.
struct PeerBridge {
    peer: Arc<MobileMultiStageSession>,
    recorder: Arc<RecordingListener>,
}

impl MultiStageSessionListener for PeerBridge {
    fn on_qr_payload(&self, payload: MobileQrPayload) {
        let _ = self.peer.process_scanned_qr(payload.data);
    }

    fn on_state_changed(&self, _state: MobileProtocolState) {}

    fn on_finalized(&self, contact_name: String) {
        self.recorder
            .finalized_names
            .lock()
            .expect("finalized_names")
            .push(contact_name);
    }

    fn on_session_ended(&self) {
        *self.recorder.session_ended.lock().expect("session_ended") = true;
    }
}

fn new_platform(name: &str) -> (Arc<VauchiPlatform>, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let wb = VauchiPlatform::new(
        dir.path().to_string_lossy().to_string(),
        "http://localhost:8080".to_string(),
    )
    .expect("VauchiPlatform::new");
    wb.create_identity(name.to_string())
        .expect("create_identity");
    (wb, dir)
}

fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while Instant::now().duration_since(start) < timeout {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    predicate()
}

// @internal
//
// Phase 2.5 contract test — the cycle thread persists the peer contact
// (and ratchet) at the `Finalized` transition before firing
// `on_finalized`. RED on main pre-Phase-2.5; green once
// `MobileMultiStageSession::with_persistence` is wired.
#[test]
fn listener_path_persists_peer_contact_after_finalized() {
    let (alice, _alice_dir) = new_platform("Alice");
    let (bob, _bob_dir) = new_platform("Bob");

    // Pre-condition: neither side has any contacts yet.
    assert_eq!(
        alice.list_contacts().expect("alice list").len(),
        0,
        "Alice must start with no contacts"
    );
    assert_eq!(
        bob.list_contacts().expect("bob list").len(),
        0,
        "Bob must start with no contacts"
    );

    let alice_session = alice
        .create_multistage_session()
        .expect("alice create_multistage_session");
    let bob_session = bob
        .create_multistage_session()
        .expect("bob create_multistage_session");

    let alice_recorder = RecordingListener::new();
    let bob_recorder = RecordingListener::new();

    // Each side's listener bridges QR payloads into the peer. Use the
    // 5 ms sleep override so the test completes well within the
    // default cargo-nextest 60 s budget even on a slow host.
    alice_session.set_cycle_sleep_override_ms_for_test(5);
    bob_session.set_cycle_sleep_override_ms_for_test(5);

    alice_session.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&bob_session),
        recorder: Arc::clone(&alice_recorder),
    }));
    bob_session.set_listener(Box::new(PeerBridge {
        peer: Arc::clone(&alice_session),
        recorder: Arc::clone(&bob_recorder),
    }));

    alice_session.start();
    bob_session.start();

    // Both `on_finalized` callbacks must fire before we can assert
    // persistence. Local: ~4.5 s with the 5 ms cycle override. CI is
    // an 8x slower / more contended runner, so the original 10 s
    // budget under-allocates and trips on shared-runner pipelines
    // (vauchi/core!747 timed out; same test passed locally 5/5 at
    // 4.4 s each). 30 s gives ~6x margin over local; CI runs that
    // exceed this point to a real protocol regression rather than
    // runner contention.
    let finalized = wait_until(Duration::from_secs(30), || {
        alice_recorder.finalized_name().is_some() && bob_recorder.finalized_name().is_some()
    });
    assert!(
        finalized,
        "exchange did not finalize on both sides within 30 s; alice={:?} bob={:?}",
        alice_recorder.finalized_name(),
        bob_recorder.finalized_name()
    );

    // Drain the grace period so the cycle thread emits its final
    // `on_session_ended` and the listener has nothing else to do.
    alice_session.cancel();
    bob_session.cancel();
    let _ = wait_until(Duration::from_secs(2), || {
        alice_recorder.session_ended() && bob_recorder.session_ended()
    });

    // The contract Phase 2.5 must satisfy:
    //
    // 1. Each peer ends up in the other's `list_contacts()` exactly
    //    once with the expected display name.
    // 2. A ratchet state row exists for the new contact id (proxy:
    //    saving the contact also initialises the ratchet, so a
    //    `list_contacts` hit for an exchanged contact implies the
    //    ratchet is in place per `VauchiPlatform::finalize_exchange`
    //    semantics. We assert via `list_contacts` here and add a
    //    ratchet-specific assertion when the production path makes
    //    it accessible).
    let alice_contacts = alice.list_contacts().expect("alice list_contacts");
    let bob_contacts = bob.list_contacts().expect("bob list_contacts");

    assert_eq!(
        alice_contacts.len(),
        1,
        "Alice should have Bob persisted after on_finalized; found {alice_contacts:?}"
    );
    assert_eq!(
        bob_contacts.len(),
        1,
        "Bob should have Alice persisted after on_finalized; found {bob_contacts:?}"
    );
    assert_eq!(
        alice_contacts[0].display_name, "Bob",
        "Alice's saved contact must carry Bob's display name"
    );
    assert_eq!(
        bob_contacts[0].display_name, "Alice",
        "Bob's saved contact must carry Alice's display name"
    );
}
