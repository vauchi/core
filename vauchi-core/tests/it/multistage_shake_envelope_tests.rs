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
//!
//! The same bound pair also checks the hover handshake's audio-response
//! verification and the link layout a settled session reports.

use vauchi_core::exchange::multistage::accel_envelope::seal_envelope;
use vauchi_core::exchange::multistage::qr_codec::{
    StageQr, format_ini2_qr_with_relay, format_shake_qr, parse_qr,
};
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
    // Listening from peer discovery, as the engine does: a session still
    // recording holds a peer CONF, so it stays in Confirming to swap its
    // envelope (#315).
    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

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

// The hover handshake's audio response must equal the peer's session id;
// before Stage 1 binds a peer there is nothing to compare against.
// @internal
#[test]
fn an_audio_response_verifies_only_as_the_bound_peer_s_session_id() {
    let fresh = MultiStageSession::new(b"name:Alice".to_vec());
    assert_eq!(fresh.verify_audio_response(&[0u8; 16]), None);

    let (alice, bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());
    let bob_id = bob.session_id();
    let mut wrong = bob_id;
    wrong[15] ^= 1;

    assert_eq!(alice.verify_audio_response(&bob_id), Some(true));
    assert_eq!(alice.verify_audio_response(&wrong), Some(false));
    assert_eq!(alice.verify_audio_response(&bob_id[..15]), Some(false));
    assert_eq!(
        alice.verify_audio_response(&alice.session_id()),
        Some(false)
    );
}

// The layout the peer last read is what a settled session keeps showing,
// and what the next session on this device starts from.
// @internal
#[test]
fn a_settled_session_reports_the_layout_it_keeps_showing() {
    let mut alice = MultiStageSession::new(b"name:Alice".to_vec()).with_start_layout(7);
    let mut bob = MultiStageSession::new(b"name:Bob".to_vec()).with_start_layout(7);
    assert_eq!(alice.last_good_layout(), None);

    for _ in 0..2000 {
        if let Some(aq) = alice.get_display_qr() {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        if alice.get_transport_key().is_some() {
            break;
        }
    }

    let settled = alice.last_good_layout().expect("bob echoed a layout");
    assert_eq!(settled, 7);
    assert_eq!(alice.get_display_qr().unwrap().layout, settled);
}

/// Alice in Confirming with a transport key and the shake stage listening,
/// but no samples recorded yet; Bob is her bound peer.
fn alice_listening_without_samples() -> (MultiStageSession, MultiStageSession) {
    let mut alice = MultiStageSession::new(b"name:Alice".to_vec());
    let mut bob = MultiStageSession::new(b"name:Bob".to_vec());
    alice
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();
    for _ in 0..2000 {
        if let Some(aq) = alice.get_display_qr() {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        if alice.get_state() == ProtocolState::Confirming && alice.get_transport_key().is_some() {
            return (alice, bob);
        }
    }
    panic!("Alice never reached Confirming with a transport_key");
}

// Listening is not enough to send or judge a shake: with nothing recorded
// there is no envelope to seal and nothing to correlate a peer's against.
// @internal
#[test]
fn a_listening_session_with_no_samples_neither_sends_nor_judges_a_shake() {
    let (mut alice, bob) = alice_listening_without_samples();
    assert_eq!(alice.accel_envelope_len(), 0);

    for _ in 0..14 {
        if let Some(p) = alice.get_display_qr() {
            assert!(!matches!(parse_qr(&p.data), Ok(StageQr::Shake { .. })));
        }
    }
    alice.process_scanned_qr(&peer_shake_qr(&bob, &shake_impulse()));

    assert_eq!(
        alice.accel_proximity(),
        AccelerometerProximityState::Listening
    );
}

// @internal
#[test]
fn the_shake_envelope_grows_by_the_samples_recorded_while_listening() {
    let (mut alice, _bob) = alice_listening_without_samples();

    alice.record_accel_envelope_samples(&shake_impulse());
    alice.record_accel_envelope_samples(&[1.0, 2.0, 3.0]);

    assert_eq!(alice.accel_envelope_len(), shake_impulse().len() + 3);
}

// Each listening stage gives up exactly when its timeout has elapsed:
// 5 seconds for audio, 8 for the accelerometer.
// @internal
#[test]
fn listening_stages_time_out_exactly_at_their_limit() {
    use std::sync::Arc;
    use std::time::Duration;
    use vauchi_core::exchange::multistage::types::AudioProximityState;
    use vauchi_core::monotonic::{FakeMonotonicClock, MonotonicClock};

    let clock = Arc::new(FakeMonotonicClock::new());
    let mut session = MultiStageSession::new(b"name:Alice".to_vec()).with_monotonic(clock.clone());
    let started = clock.now();
    session
        .set_audio_proximity(AudioProximityState::Listening)
        .unwrap();
    session
        .set_accel_proximity(AccelerometerProximityState::Listening)
        .unwrap();

    let audio_limit = started + Duration::from_secs(5);
    assert!(
        !session
            .check_and_apply_audio_timeout(audio_limit - Duration::from_millis(1))
            .unwrap()
    );
    assert!(session.check_and_apply_audio_timeout(audio_limit).unwrap());

    let accel_limit = started + Duration::from_secs(8);
    assert!(
        !session
            .check_and_apply_accel_timeout(accel_limit - Duration::from_millis(1))
            .unwrap()
    );
    assert!(session.check_and_apply_accel_timeout(accel_limit).unwrap());
}

// INID (INIT with the whole payload) is no longer sent, but a peer that sends
// one still completes Stage 1 and delivers its whole payload in a single
// frame; we then only have our own chunks left to send.
// @internal
#[test]
fn a_received_inid_frame_delivers_the_peer_payload_in_one_read() {
    use vauchi_core::exchange::X3DHKeyPair;
    use vauchi_core::exchange::multistage::commitment::Commitment;
    use vauchi_core::exchange::multistage::qr_codec::format_in2d_qr;

    let mut bob = MultiStageSession::new(b"name:Bob".to_vec());
    bob.get_display_qr().unwrap();
    let peer_sid = [5u8; 16];
    let commitment = Commitment::create(b"name:Peer").unwrap();
    let inid = format_in2d_qr(
        &peer_sid,
        X3DHKeyPair::generate().public_key(),
        commitment.hash(),
        "Peer",
        None,
        commitment.ciphertext(),
    );

    bob.process_scanned_qr(&inid);

    assert_eq!(bob.peer_session_id(), Some(peer_sid));
    assert!(
        matches!(
            bob.get_state(),
            ProtocolState::Transferring {
                chunks_received: 1,
                peer_chunks_total: 1,
                ..
            }
        ),
        "{:?}",
        bob.get_state()
    );
}

/// Runs an exchange where Alice advertises `https://alice.relay` and Bob
/// reads her first INIT with its relay URL replaced by `bob_sees` (or as
/// sent, when `None`). Returns Bob's state once he fails, finalizes, or the
/// exchange stops moving.
fn exchange_with_relay_seen_by_bob(bob_sees: Option<&str>) -> ProtocolState {
    let mut alice = MultiStageSession::new_with_relay(
        b"name:Alice".to_vec(),
        Some("https://alice.relay".into()),
    );
    let mut bob = MultiStageSession::new(b"name:Bob".to_vec());
    let alice_init = alice.get_display_qr().unwrap().data;
    let bob_init = bob.get_display_qr().unwrap().data;
    let seen = match (parse_qr(&alice_init), bob_sees) {
        (
            Ok(StageQr::Init {
                session_id,
                ephemeral,
                commitment_hash,
                display_name,
                ..
            }),
            Some(url),
        ) => format_ini2_qr_with_relay(
            &session_id,
            &ephemeral,
            &commitment_hash,
            &display_name,
            Some(url),
        ),
        _ => alice_init,
    };
    bob.process_scanned_qr(&seen);
    alice.process_scanned_qr(&bob_init);

    for _ in 0..2000 {
        if let Some(bq) = bob.get_display_qr() {
            alice.process_scanned_qr(&bq.data);
        }
        if let Some(aq) = alice.get_display_qr() {
            if matches!(parse_qr(&aq.data), Ok(StageQr::Init { .. })) {
                continue; // Bob keeps the relay URL he first read
            }
            bob.process_scanned_qr(&aq.data);
        }
        if matches!(
            bob.get_state(),
            ProtocolState::Failed(_) | ProtocolState::Finalized
        ) {
            break;
        }
    }
    bob.get_state()
}

// T1.7: the commitment binds the advertised relay URL, so a relay URL
// rewritten in transit makes the peer's commitment fail to verify.
// @internal
#[test]
fn a_relay_url_rewritten_in_transit_fails_the_commitment() {
    assert_eq!(
        exchange_with_relay_seen_by_bob(None),
        ProtocolState::Finalized
    );
    assert!(
        matches!(
            exchange_with_relay_seen_by_bob(Some("https://evil.relay")),
            ProtocolState::Failed(_)
        ),
        "a swapped relay URL must not verify"
    );
}

// A DATA chunk shorter than its 12-byte nonce plus 16-byte tag is counted as
// a failed decrypt, not split and read past its end.
// @internal
#[test]
fn a_data_chunk_shorter_than_nonce_and_tag_counts_as_a_decrypt_failure() {
    use vauchi_core::exchange::multistage::qr_codec::format_data_qr;

    let (alice, mut bob) = drive_to_transport_key(b"name:Alice".to_vec(), b"name:Bob".to_vec());
    let before = bob.transport_decrypt_failure_count();

    bob.process_scanned_qr(&format_data_qr(
        &alice.session_id(),
        0,
        1,
        &[],
        &[1, 2, 3, 4, 5],
    ));

    assert_eq!(bob.transport_decrypt_failure_count(), before + 1);
}
