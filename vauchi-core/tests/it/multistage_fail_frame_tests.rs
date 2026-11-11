// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! FAIL (vauchi/private#1217): a phone that rejects its peer's payload, or
//! whose app could not save the peer, shows FAIL for a short linger so the
//! peer stops at once instead of waiting out its timeout. Only the phone
//! bound to as the peer can end an exchange this way; a stray FAIL from
//! anyone else changes nothing.

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::exchange::multistage::qr_codec::{self, StageQr, parse_qr};
use vauchi_core::exchange::multistage::session::{FAIL_LINGER, MultiStageSession};
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::monotonic::FakeMonotonicClock;
use vauchi_core::{ContactCard, Identity};

const TICK: Duration = Duration::from_millis(100);

fn signed(identity: &Identity, clock: &Arc<FakeMonotonicClock>) -> MultiStageSession {
    MultiStageSession::for_identity(identity, &ContactCard::new("Someone"), None)
        .with_monotonic(clock.clone())
}

/// Each phone reads the other's frames until `stop` holds.
fn run(
    a: &mut MultiStageSession,
    b: &mut MultiStageSession,
    clock: &FakeMonotonicClock,
    stop: impl Fn(&MultiStageSession, &MultiStageSession) -> bool,
) {
    let ai = a.get_display_qr().unwrap();
    let bi = b.get_display_qr().unwrap();
    a.process_scanned_qr(&bi.data);
    b.process_scanned_qr(&ai.data);
    for _ in 0..400 {
        if stop(a, b) {
            return;
        }
        if let Some(q) = a.get_display_qr() {
            b.process_scanned_qr(&q.data);
        }
        if let Some(q) = b.get_display_qr() {
            a.process_scanned_qr(&q.data);
        }
        clock.advance(TICK);
    }
    panic!("never stopped: {:?} / {:?}", a.get_state(), b.get_state());
}

fn is_fail(frame: &str) -> bool {
    matches!(parse_qr(frame), Ok(StageQr::Fail { .. }))
}

/// Two devices of one identity: each rejects the other's payload.
fn twins() -> (
    MultiStageSession,
    MultiStageSession,
    Arc<FakeMonotonicClock>,
) {
    let clock = Arc::new(FakeMonotonicClock::new());
    let alice = Identity::create("Alice", 0);
    let mut a = signed(&alice, &clock);
    let mut twin = signed(&alice, &clock);
    run(&mut a, &mut twin, &clock, |a, _| {
        matches!(a.get_state(), ProtocolState::Failed(_))
    });
    (a, twin, clock)
}

// @internal
#[test]
fn a_phone_that_rejects_its_peer_shows_fail_for_the_linger_and_then_nothing() {
    let (mut a, _twin, clock) = twins();

    let frame = a
        .get_display_qr()
        .expect("a rejecting phone shows FAIL")
        .data;
    assert!(is_fail(&frame), "showed {}", &frame[..4]);
    assert_eq!(
        parse_qr(&frame).unwrap().session_id(),
        &a.session_id(),
        "FAIL names the sender's own session"
    );
    assert!(a.shows_fail_frame());

    clock.advance(FAIL_LINGER - TICK);
    assert!(a.get_display_qr().is_some_and(|q| is_fail(&q.data)));
    clock.advance(TICK);
    assert_eq!(a.get_display_qr().map(|q| q.data), None);
    assert!(!a.shows_fail_frame());
}

// @internal
#[test]
fn the_peer_that_reads_fail_from_its_bound_peer_stops() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let alice = Identity::create("Alice", 0);
    let mut a = signed(&alice, &clock);
    let mut twin = signed(&alice, &clock);

    run(&mut a, &mut twin, &clock, |a, b| {
        matches!(a.get_state(), ProtocolState::Failed(_))
            && matches!(b.get_state(), ProtocolState::Failed(_))
    });

    for session in [&a, &twin] {
        assert!(session.received_peer().is_none());
    }
}

// @internal
#[test]
fn a_fail_from_a_session_other_than_the_bound_peer_changes_nothing() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut a = signed(&Identity::create("Alice", 0), &clock);
    let mut b = signed(&Identity::create("Bob", 0), &clock);
    let stranger = qr_codec::format_fail_qr(&[0xEE; 16]);

    a.get_display_qr();
    let advertising = a.process_scanned_qr(&stranger);
    assert_eq!(advertising, ProtocolState::Advertising, "no peer bound yet");

    let bi = b.get_display_qr().unwrap();
    a.process_scanned_qr(&bi.data);
    let bound = a.get_state();
    assert_eq!(a.process_scanned_qr(&stranger), bound, "a bystander's FAIL");
}

// @internal
#[test]
fn a_phone_whose_app_could_not_save_shows_fail_and_its_peer_learns_it() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut a = signed(&Identity::create("Alice", 0), &clock);
    let mut b = signed(&Identity::create("Bob", 0), &clock);
    // Alice finalizes first: Bob never reads her confirm.
    let ai = a.get_display_qr().unwrap();
    let bi = b.get_display_qr().unwrap();
    a.process_scanned_qr(&bi.data);
    b.process_scanned_qr(&ai.data);
    for _ in 0..400 {
        if a.get_state() == ProtocolState::Finalized {
            break;
        }
        if let Some(q) = a.get_display_qr()
            && !q.data.starts_with("CNF4")
        {
            b.process_scanned_qr(&q.data);
        }
        if let Some(q) = b.get_display_qr() {
            a.process_scanned_qr(&q.data);
        }
        clock.advance(TICK);
    }
    assert_eq!(a.get_state(), ProtocolState::Finalized);
    assert_eq!(b.get_state(), ProtocolState::Confirming);

    a.save_failed();

    assert!(matches!(a.get_state(), ProtocolState::Failed(_)));
    let frame = a.get_display_qr().expect("FAIL is shown").data;
    assert!(is_fail(&frame), "showed {}", &frame[..4]);
    b.process_scanned_qr(&frame);
    assert!(
        matches!(b.get_state(), ProtocolState::Failed(_)),
        "Bob stops instead of timing out: {:?}",
        b.get_state()
    );
}

// @internal
#[test]
fn a_finalized_phone_ignores_its_peers_fail() {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut a = signed(&Identity::create("Alice", 0), &clock);
    let mut b = signed(&Identity::create("Bob", 0), &clock);
    run(&mut a, &mut b, &clock, |a, b| {
        a.get_state() == ProtocolState::Finalized && b.get_state() == ProtocolState::Finalized
    });

    b.save_failed();
    let fail = b.get_display_qr().expect("FAIL is shown").data;

    assert_eq!(a.process_scanned_qr(&fail), ProtocolState::Finalized);
}
