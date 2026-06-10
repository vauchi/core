// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use std::time::{Duration, Instant};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::session::{AccelStateError, AudioStateError};
use vauchi_core::exchange::multistage::types::{
    AccelerometerProximityState, AudioProximityState, ProtocolState,
};

// @internal
#[test]
fn test_new_session_is_idle() {
    let card = b"Alice's contact card".to_vec();
    let session = MultiStageSession::new(card);
    assert!(matches!(session.get_state(), ProtocolState::Idle));
}

// @internal
#[test]
fn test_new_session_audio_proximity_starts_pending() {
    // Phase 1.C.3a foothold — every freshly-constructed session
    // begins with audio_proximity=Pending. Glance never transitions
    // it; Hover drives it through the ultrasonic handshake states
    // (Phase 1.C.3b).
    let card = b"Alice's contact card".to_vec();
    let session = MultiStageSession::new(card);
    assert_eq!(session.audio_proximity(), AudioProximityState::Pending);
}

// @internal
#[test]
fn test_new_with_relay_session_audio_proximity_starts_pending() {
    // The relay constructor takes a different path through
    // new_with_relay; assert the field is initialised on this path
    // too so a future refactor that drops the second initialiser
    // doesn't silently regress to a stale Default.
    let card = b"Alice's contact card".to_vec();
    let session =
        MultiStageSession::new_with_relay(card, Some("https://relay.example/route".to_string()));
    assert_eq!(session.audio_proximity(), AudioProximityState::Pending);
}

// @internal
#[test]
fn test_get_display_qr_starts_advertising() {
    let card = b"Alice's card".to_vec();
    let mut session = MultiStageSession::new(card);
    let qr = session.get_display_qr();
    let qr = qr.expect("expected Some");
    assert!(qr.data.starts_with("INI2"));
    assert!(matches!(session.get_state(), ProtocolState::Advertising));
}

// @internal
#[test]
fn test_full_exchange_two_sessions() {
    let alice_card = b"Alice's full contact card with avatar data".to_vec();
    let bob_card = b"Bob's full contact card with avatar data".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    // Stage 1: Both display INIT QRs
    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    assert!(alice_init.data.starts_with("INI2"));
    assert!(bob_init.data.starts_with("INI2"));

    let alice_state = alice.process_scanned_qr(&bob_init.data);
    let bob_state = bob.process_scanned_qr(&alice_init.data);
    assert!(matches!(
        alice_state,
        ProtocolState::Discovered | ProtocolState::Transferring { .. }
    ));
    assert!(matches!(
        bob_state,
        ProtocolState::Discovered | ProtocolState::Transferring { .. }
    ));

    // Stage 2: Exchange DATA chunks until both complete
    for _ in 0..100 {
        let alice_qr = alice.get_display_qr();
        let bob_qr = bob.get_display_qr();

        if let Some(aq) = &alice_qr {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bob_qr {
            alice.process_scanned_qr(&bq.data);
        }

        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(matches!(bob.get_state(), ProtocolState::Finalized));

    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}

// @internal
#[test]
fn test_full_exchange_with_relay_metadata() {
    let alice_card = b"Alice's card for relay test".to_vec();
    let bob_card = b"Bob's card for relay test".to_vec();

    let mut alice = MultiStageSession::new_with_relay(
        alice_card.clone(),
        Some("https://alice-relay.example.com".to_string()),
    );
    let mut bob = MultiStageSession::new_with_relay(
        bob_card.clone(),
        Some("https://bob-relay.example.com".to_string()),
    );

    // Stage 1: Both display INIT QRs
    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();

    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(matches!(bob.get_state(), ProtocolState::Finalized));

    assert_eq!(
        alice.peer_relay_url(),
        Some("https://bob-relay.example.com")
    );

    assert_eq!(
        bob.peer_relay_url(),
        Some("https://alice-relay.example.com")
    );

    assert_eq!(alice.get_received_data().unwrap(), bob_card);
    assert_eq!(bob.get_received_data().unwrap(), alice_card);
}

// @internal
#[test]
fn test_exchange_without_relay_metadata() {
    let alice_card = b"Alice no relay".to_vec();
    let bob_card = b"Bob no relay".to_vec();

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    for _ in 0..100 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    assert!(alice.peer_relay_url().is_none());
    assert!(bob.peer_relay_url().is_none());
}

// @internal
#[test]
fn test_cancel_session_clears_state() {
    let mut session = MultiStageSession::new(b"data".to_vec());
    session.get_display_qr();
    session.cancel();
    assert!(matches!(session.get_state(), ProtocolState::Failed(_)));
    assert!(session.get_received_data().is_none());
}

// @internal
#[test]
fn test_exchange_with_large_payload() {
    let alice_card = vec![0xAA; 15_000];
    let bob_card = vec![0xBB; 15_000];

    let mut alice = MultiStageSession::new(alice_card.clone());
    let mut bob = MultiStageSession::new(bob_card.clone());

    let alice_init = alice.get_display_qr().unwrap();
    let bob_init = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bob_init.data);
    bob.process_scanned_qr(&alice_init.data);

    for _ in 0..1000 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            break;
        }
    }

    assert!(matches!(alice.get_state(), ProtocolState::Finalized));
    let alice_received = alice.get_received_data().unwrap();
    let bob_received = bob.get_received_data().unwrap();
    assert_eq!(alice_received, bob_card);
    assert_eq!(bob_received, alice_card);
}

// ── Audio proximity state machine (Phase 1.C.3b) ──────────────────

// @internal
#[test]
fn test_audio_proximity_pending_to_listening_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    assert_eq!(s.audio_proximity(), AudioProximityState::Pending);
    s.set_audio_proximity(AudioProximityState::Listening)
        .expect("Pending → Listening must be allowed");
    assert_eq!(s.audio_proximity(), AudioProximityState::Listening);
}

// @internal
#[test]
fn test_audio_proximity_listening_to_confirmed_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    s.set_audio_proximity(AudioProximityState::Confirmed)
        .expect("Listening → Confirmed must be allowed");
    assert_eq!(s.audio_proximity(), AudioProximityState::Confirmed);
}

// @internal
#[test]
fn test_audio_proximity_listening_to_failed_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    s.set_audio_proximity(AudioProximityState::Failed)
        .expect("Listening → Failed must be allowed");
    assert_eq!(s.audio_proximity(), AudioProximityState::Failed);
}

// @internal
#[test]
fn test_audio_proximity_failed_to_listening_allowed_for_retry() {
    // G1.3 of the Hover graduation problem record: retry restarts the
    // audio verifier without restarting the QR cycle. The session
    // permits Failed → Listening so the wrapper's retry handler can
    // re-arm the handshake.
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    s.set_audio_proximity(AudioProximityState::Failed).unwrap();
    s.set_audio_proximity(AudioProximityState::Listening)
        .expect("Failed → Listening (retry) must be allowed");
    assert_eq!(s.audio_proximity(), AudioProximityState::Listening);
}

// @internal
#[test]
fn test_audio_proximity_pending_to_confirmed_rejected_as_security_gate() {
    // Security gate: Confirmed claims the devices are physically
    // close. That claim is only valid after a verified ultrasonic
    // exchange. Skipping the Listening window must be rejected.
    let mut s = MultiStageSession::new(b"card".to_vec());
    let err = s
        .set_audio_proximity(AudioProximityState::Confirmed)
        .expect_err("Pending → Confirmed must be rejected");
    assert_eq!(
        err,
        AudioStateError::InvalidTransition {
            from: AudioProximityState::Pending,
            to: AudioProximityState::Confirmed,
        }
    );
    assert_eq!(s.audio_proximity(), AudioProximityState::Pending);
}

// @internal
#[test]
fn test_audio_proximity_pending_to_failed_rejected() {
    // Failed without having tried Listening makes no sense — the
    // wrapper must have entered the chirp/listen window before it
    // can report a failure on it. Reject as a caller bug.
    let mut s = MultiStageSession::new(b"card".to_vec());
    let err = s
        .set_audio_proximity(AudioProximityState::Failed)
        .expect_err("Pending → Failed must be rejected");
    assert!(matches!(err, AudioStateError::InvalidTransition { .. }));
    assert_eq!(s.audio_proximity(), AudioProximityState::Pending);
}

// @internal
#[test]
fn test_audio_proximity_confirmed_is_terminal_success() {
    // transition out of it within a session. (A new session resets
    // to Pending via construction.)
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    s.set_audio_proximity(AudioProximityState::Confirmed)
        .unwrap();
    assert!(
        s.set_audio_proximity(AudioProximityState::Listening)
            .is_err()
    );
    assert!(s.set_audio_proximity(AudioProximityState::Failed).is_err());
    assert!(s.set_audio_proximity(AudioProximityState::Pending).is_err());
    assert_eq!(s.audio_proximity(), AudioProximityState::Confirmed);
}

// @internal
#[test]
fn test_audio_proximity_listening_self_transition_rejected() {
    // No-op self-transitions are surfaced as errors so the wrapper
    // can detect a duplicate state-change attempt rather than
    // silently ignoring it.
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    assert!(
        s.set_audio_proximity(AudioProximityState::Listening)
            .is_err()
    );
}

// ── Audio-listen timeout (Phase 1.C.6/7) ───────────────────────────
//
// RED tests for Phase 1.C.6 of
// `_private/docs/planning/todo/2026-05-11-hover-graduation-plan.md`.
// `set_audio_proximity(Listening)` opens a listen window with a
// fixed budget (`Self::AUDIO_LISTEN_TIMEOUT`). If no audio response
// arrives within that budget the session must transition to
// `Failed` so the renderer can surface "Couldn't confirm devices
// are close" (Hover problem record G1.3) instead of leaving the
// user staring at a Listening spinner forever.
//
// The check runs against an injected `Instant` per the project's
// CC-06 "no real-time waits" rule — tests offset against
// `Instant::now()` rather than sleeping.
//
// All three tests are compile-RED: `check_and_apply_audio_timeout`
// doesn't exist yet (Phase 1.C.7 GREEN adds it).

// @internal
#[test]
fn audio_timeout_transitions_listening_to_failed_after_budget() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    let entered = Instant::now();
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();

    // Listen budget is 5s (mirror of the platform-layer
    // AUDIO_LISTEN_TIMEOUT_MS constant). Past the deadline → Failed.
    let after = entered + Duration::from_secs(6);
    let fired = s
        .check_and_apply_audio_timeout(after)
        .expect("timeout check must succeed on a Listening session");
    assert!(fired, "timeout past 5s budget must fire Listening → Failed");
    assert_eq!(s.audio_proximity(), AudioProximityState::Failed);
}

// @internal
#[test]
fn audio_timeout_set_site_uses_injected_monotonic_clock() {
    use std::sync::Arc;
    use vauchi_core::monotonic::{FakeMonotonicClock, MonotonicClock};

    // Phase 1 / Task 1.1b: `set_audio_proximity` stamps the listen-window
    // start via `self.monotonic.now()`. With an injected fake clock, the
    // recorded start and the timeout `now` share one controlled domain,
    // so advancing the fake clock alone fires the 5s budget — no real
    // wait, and no dependence on ambient `Instant::now()`.
    let fake = Arc::new(FakeMonotonicClock::new());
    let mut s = MultiStageSession::new(b"card".to_vec()).with_monotonic(fake.clone());
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();

    assert!(
        !s.check_and_apply_audio_timeout(fake.now()).unwrap(),
        "fresh listen window (fake at offset 0) must not time out"
    );

    fake.advance(Duration::from_secs(6));
    assert!(
        s.check_and_apply_audio_timeout(fake.now()).unwrap(),
        "advancing the injected clock past the 5s budget must fire the timeout"
    );
    assert_eq!(s.audio_proximity(), AudioProximityState::Failed);
}

// @internal
#[test]
fn audio_timeout_no_op_before_budget() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    let entered = Instant::now();
    s.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();

    // Before deadline → no transition, still Listening.
    let early = entered + Duration::from_secs(3);
    let fired = s
        .check_and_apply_audio_timeout(early)
        .expect("timeout check must succeed on a Listening session");
    assert!(!fired, "timeout check before 5s must NOT fire");
    assert_eq!(s.audio_proximity(), AudioProximityState::Listening);
}

// @internal
#[test]
fn audio_timeout_no_op_when_not_listening() {
    // Pending → check is a no-op (no listen window open).
    let mut s = MultiStageSession::new(b"card".to_vec());
    let after = Instant::now() + Duration::from_secs(60);
    let fired = s
        .check_and_apply_audio_timeout(after)
        .expect("timeout check on non-Listening session must succeed");
    assert!(!fired, "Pending must not transition to Failed via timeout");
    assert_eq!(s.audio_proximity(), AudioProximityState::Pending);

    // Confirmed → also a no-op (handshake already verified, no
    // window to close).
    let mut s2 = MultiStageSession::new(b"card".to_vec());
    s2.set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    s2.set_audio_proximity(AudioProximityState::Confirmed)
        .unwrap();
    let later = Instant::now() + Duration::from_secs(60);
    let fired = s2
        .check_and_apply_audio_timeout(later)
        .expect("timeout check on Confirmed must succeed");
    assert!(!fired, "Confirmed must not regress via timeout");
    assert_eq!(s2.audio_proximity(), AudioProximityState::Confirmed);
}

// ── Accelerometer-proximity state machine (P2.A — TapHoverShake) ────
//
// TapHoverShake graduation Phase 2.A
// (`_private/docs/planning/todo/2026-06-03-taphovershake-graduation-plan.md`).
// A *second* parallel proximity signal alongside `audio_proximity`,
// mirroring its exact state graph and timeout discipline. The session
// owns the protocol state (ADR-043); the orchestrator (Phase 2.C)
// drives it from `Event::AccelerometerData` ingress + the peer envelope
// exchanged over transport. These tests pin the state machine alone —
// no caller yet (Glance and Hover never transition the field; only
// TapHoverShake will).

// @internal
#[test]
fn test_new_session_accel_proximity_starts_pending() {
    // Every freshly-constructed session begins with
    // accel_proximity=Pending. Glance and Hover never transition it;
    // only TapHoverShake drives it through the shake-correlation states.
    let card = b"Alice's contact card".to_vec();
    let session = MultiStageSession::new(card);
    assert_eq!(
        session.accel_proximity(),
        AccelerometerProximityState::Pending
    );
}

// @internal
#[test]
fn test_new_with_relay_session_accel_proximity_starts_pending() {
    // The relay constructor takes a different path through
    // new_with_relay; assert the field is initialised on this path too
    // so a future refactor that drops the second initialiser doesn't
    // silently regress to a stale Default.
    let card = b"Alice's contact card".to_vec();
    let session =
        MultiStageSession::new_with_relay(card, Some("https://relay.example/route".to_string()));
    assert_eq!(
        session.accel_proximity(),
        AccelerometerProximityState::Pending
    );
}

// @internal
#[test]
fn test_accel_proximity_pending_to_listening_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Pending);
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Listening);
}

// @internal
#[test]
fn test_accel_proximity_listening_to_confirmed_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    s.set_accel_proximity(AccelerometerProximityState::Confirmed)
        .unwrap();
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Confirmed);
}

// @internal
#[test]
fn test_accel_proximity_listening_to_failed_allowed() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    s.set_accel_proximity(AccelerometerProximityState::Failed)
        .unwrap();
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Failed);
}

// @internal
#[test]
fn test_accel_proximity_failed_to_listening_allowed_for_retry() {
    // Retry restarts the shake capture without restarting the QR cycle,
    // mirroring the audio signal's Failed → Listening retry edge.
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    s.set_accel_proximity(AccelerometerProximityState::Failed)
        .unwrap();
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .expect("Failed → Listening (retry) must be allowed");
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Listening);
}

// @internal
#[test]
fn test_accel_proximity_pending_to_confirmed_rejected_as_security_gate() {
    // Security gate: Confirmed claims the devices experienced the same
    // physical impulse (cross-correlated envelopes). That claim is only
    // valid after a recording window. Skipping Listening must reject.
    let mut s = MultiStageSession::new(b"card".to_vec());
    let err = s
        .set_accel_proximity(AccelerometerProximityState::Confirmed)
        .expect_err("Pending → Confirmed must be rejected");
    assert_eq!(
        err,
        AccelStateError::InvalidTransition {
            from: AccelerometerProximityState::Pending,
            to: AccelerometerProximityState::Confirmed,
        }
    );
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Pending);
}

// @internal
#[test]
fn test_accel_proximity_pending_to_failed_rejected() {
    // Failed without having entered Listening makes no sense — the
    // orchestrator must have opened the capture window before it can
    // report a failure on it. Reject as a caller bug.
    let mut s = MultiStageSession::new(b"card".to_vec());
    let err = s
        .set_accel_proximity(AccelerometerProximityState::Failed)
        .expect_err("Pending → Failed must be rejected");
    assert!(matches!(err, AccelStateError::InvalidTransition { .. }));
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Pending);
}

// @internal
#[test]
fn test_accel_proximity_confirmed_is_terminal_success() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    s.set_accel_proximity(AccelerometerProximityState::Confirmed)
        .unwrap();
    assert!(
        s.set_accel_proximity(AccelerometerProximityState::Listening)
            .is_err()
    );
    assert!(
        s.set_accel_proximity(AccelerometerProximityState::Failed)
            .is_err()
    );
    assert!(
        s.set_accel_proximity(AccelerometerProximityState::Pending)
            .is_err()
    );
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Confirmed);
}

// @internal
#[test]
fn test_accel_proximity_listening_self_transition_rejected() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    assert!(
        s.set_accel_proximity(AccelerometerProximityState::Listening)
            .is_err()
    );
}

// ── Accelerometer-capture timeout (P2.A) ───────────────────────────
//
// `set_accel_proximity(Listening)` opens a capture window with a fixed
// budget (`ACCEL_LISTEN_TIMEOUT`, 8s — long enough for the 3s motion
// recording plus the peer-envelope round-trip over transport). If the
// peer envelope never cross-correlates within that budget the session
// transitions to `Failed` so the renderer surfaces "Couldn't confirm
// the shake" rather than wedging on a Recording spinner. Checked
// against an injected `Instant` per CC-06.

// @internal
#[test]
fn accel_timeout_transitions_listening_to_failed_after_budget() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    let entered = Instant::now();
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

    let after = entered + Duration::from_secs(9);
    let fired = s
        .check_and_apply_accel_timeout(after)
        .expect("timeout check must succeed on a Listening session");
    assert!(fired, "timeout past 8s budget must fire Listening → Failed");
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Failed);
}

// @internal
#[test]
fn accel_timeout_set_site_uses_injected_monotonic_clock() {
    use std::sync::Arc;
    use vauchi_core::monotonic::{FakeMonotonicClock, MonotonicClock};

    let fake = Arc::new(FakeMonotonicClock::new());
    let mut s = MultiStageSession::new(b"card".to_vec()).with_monotonic(fake.clone());
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

    assert!(
        !s.check_and_apply_accel_timeout(fake.now()).unwrap(),
        "fresh capture window (fake at offset 0) must not time out"
    );

    fake.advance(Duration::from_secs(9));
    assert!(
        s.check_and_apply_accel_timeout(fake.now()).unwrap(),
        "advancing the injected clock past the 8s budget must fire the timeout"
    );
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Failed);
}

// @internal
#[test]
fn accel_timeout_no_op_before_budget() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    let entered = Instant::now();
    s.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

    let early = entered + Duration::from_secs(3);
    let fired = s
        .check_and_apply_accel_timeout(early)
        .expect("timeout check must succeed on a Listening session");
    assert!(!fired, "timeout check before 8s must NOT fire");
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Listening);
}

// @internal
#[test]
fn accel_timeout_no_op_when_not_listening() {
    let mut s = MultiStageSession::new(b"card".to_vec());
    let after = Instant::now() + Duration::from_secs(60);
    let fired = s
        .check_and_apply_accel_timeout(after)
        .expect("timeout check on non-Listening session must succeed");
    assert!(!fired, "Pending must not transition to Failed via timeout");
    assert_eq!(s.accel_proximity(), AccelerometerProximityState::Pending);

    let mut s2 = MultiStageSession::new(b"card".to_vec());
    s2.set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    s2.set_accel_proximity(AccelerometerProximityState::Confirmed)
        .unwrap();
    let later = Instant::now() + Duration::from_secs(60);
    let fired = s2
        .check_and_apply_accel_timeout(later)
        .expect("timeout check on Confirmed must succeed");
    assert!(!fired, "Confirmed must not regress via timeout");
    assert_eq!(s2.accel_proximity(), AccelerometerProximityState::Confirmed);
}
