// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Slice-3 integration tests for the TapHoverShake accel-envelope SHAK stage.
//!
//! Drives two real `MultiStageSession`s far enough to establish the symmetric
//! `transport_key` (and each side's `peer_session_id`), then exercises the
//! advisory shake co-location signal end-to-end through the public API:
//! seal a peer envelope, feed it as a SHAK QR, and assert `accel_proximity`.
//!
//! Security-review acceptance criteria realised here: F2 (reflection
//! rejection, CC-14), F5 (drop before `transport_key`), F8 (advisory — never
//! gates completion).

use vauchi_core::exchange::multistage::accel_envelope::seal_envelope;
use vauchi_core::exchange::multistage::qr_codec::{StageQr, format_shake_qr, parse_qr};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::{AccelerometerProximityState, ProtocolState};

/// Drive both sessions until each has derived its `transport_key`, returning
/// them paused mid-exchange (before finalize, which would clear the key).
fn drive_to_transport_key(
    alice_card: Vec<u8>,
    bob_card: Vec<u8>,
) -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(alice_card);
    let mut bob = MultiStageSession::new(bob_card);

    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    for _ in 0..2000 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if alice.get_transport_key().is_some() && bob.get_transport_key().is_some() {
            return (alice, bob);
        }
    }
    panic!("transport_key was never derived on both sides");
}

/// A smooth, distinctive magnitude envelope (a co-located shake impulse).
fn shake_impulse() -> Vec<f32> {
    (0..300)
        .map(|i| {
            let t = i as f32 / 100.0;
            (t * 6.0).sin().abs() * 4.0 + (t * 2.0).cos().abs()
        })
        .collect()
}

/// Build the SHAK QR a peer would display: seal `samples` under the *peer's*
/// own session_id and transport_key (F2 sender-AAD binding).
fn peer_shake_qr(peer: &MultiStageSession, samples: &[f32]) -> String {
    let key = peer.get_transport_key().expect("peer has transport_key");
    let sealed = seal_envelope(&key, &peer.session_id(), samples);
    format_shake_qr(&peer.session_id(), &sealed)
}

// @internal
#[test]
fn shake_confirmed_when_peer_envelope_correlates() {
    let (mut alice, bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());
    let state_before = alice.get_state();

    let local = shake_impulse();
    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    alice.record_accel_envelope_samples(&local);

    // Bob recorded the *same* physical shake (co-located) → high correlation.
    let qr = peer_shake_qr(&bob, &local);
    alice.process_scanned_qr(&qr);

    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Confirmed,
        "co-located envelopes must confirm"
    );
    // F8: advisory only — the protocol state is unchanged by the SHAK.
    assert_eq!(alice.get_state(), state_before);
}

// @internal
#[test]
fn shake_failed_when_peer_envelope_uncorrelated() {
    let (mut alice, bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());

    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    alice.record_accel_envelope_samples(&shake_impulse());

    // Bob's envelope is an unrelated, near-constant signal → low correlation.
    let unrelated: Vec<f32> = (0..300).map(|i| 0.5 + (i % 3) as f32 * 0.001).collect();
    let qr = peer_shake_qr(&bob, &unrelated);
    alice.process_scanned_qr(&qr);

    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Failed,
        "uncorrelated envelopes must fail"
    );
}

// @internal
#[test]
fn shake_reflection_is_rejected_and_left_pending() {
    // CC-14 / F2: an on-path attacker reflects Alice's own envelope back to her.
    let (mut alice, _bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());

    let local = shake_impulse();
    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    alice.record_accel_envelope_samples(&local);

    // The reflected QR is sealed under ALICE's own session_id; Alice opens
    // under peer_session_id (Bob) → AEAD fails → dropped.
    let reflected = peer_shake_qr(&alice, &local);
    alice.process_scanned_qr(&reflected);

    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Listening,
        "a reflected own-envelope must be dropped (AEAD fail), leaving Listening"
    );
}

// @internal
#[test]
fn shake_before_transport_key_is_dropped() {
    // F5: a SHAK arriving before transport_key exists is dropped, not buffered
    // or errored. Fresh session: no key yet.
    let mut alice = MultiStageSession::new(b"name:Alice".to_vec());
    assert!(alice.get_transport_key().is_none());

    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    alice.record_accel_envelope_samples(&shake_impulse());

    // Some well-formed SHAK (sealed under an arbitrary key/sid the attacker holds).
    let sealed = seal_envelope(&[0x09; 32], &[0x07; 16], &shake_impulse());
    let qr = format_shake_qr(&[0x07; 16], &sealed);
    alice.process_scanned_qr(&qr);

    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Listening,
        "SHAK before transport_key must be dropped, leaving Listening"
    );
}

// @internal
#[test]
fn shake_qr_is_emitted_in_confirming_when_recording() {
    // Emit path: drive Alice to Confirming with a key, start the shake stage,
    // and confirm get_display_qr surfaces a SHAK frame within one mod-7 cycle.
    let mut alice = MultiStageSession::new(b"name:Alice".to_vec());
    let mut bob = MultiStageSession::new(b"name:Bob".to_vec());
    let ai = alice.get_display_qr().unwrap();
    let bi = bob.get_display_qr().unwrap();
    alice.process_scanned_qr(&bi.data);
    bob.process_scanned_qr(&ai.data);

    let mut reached = false;
    for _ in 0..2000 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if alice.get_state() == ProtocolState::Confirming && alice.get_transport_key().is_some() {
            reached = true;
            break;
        }
    }
    assert!(
        reached,
        "Alice never reached Confirming with a transport_key"
    );

    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    alice.record_accel_envelope_samples(&shake_impulse());

    // Poll the display cycle; phase 6 of mod-7 carries SHAK. 14 polls cover two
    // full cycles. Displaying does not transition protocol state, so Alice
    // stays in Confirming throughout.
    let mut saw_shake = false;
    for _ in 0..14 {
        if let Some(p) = alice.get_display_qr() {
            if matches!(parse_qr(&p.data), Ok(StageQr::Shake { .. })) {
                saw_shake = true;
                break;
            }
        }
    }
    assert!(
        saw_shake,
        "no SHAK frame emitted in Confirming while recording"
    );
}

// @internal
#[test]
fn no_shake_emitted_without_recording() {
    // Glance/Hover (and TapHoverShake before capture) never emit SHAK: with
    // accel_proximity Pending, build_shake_qr stays silent and the Confirming
    // cycle only ever shows VRFY/CONF.
    let (mut alice, _bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());
    // Do NOT start the shake stage. Drive display through several cycles.
    for _ in 0..21 {
        if let Some(p) = alice.get_display_qr() {
            assert!(
                !matches!(parse_qr(&p.data), Ok(StageQr::Shake { .. })),
                "SHAK emitted without an active recording"
            );
        }
    }
    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Pending
    );
}
