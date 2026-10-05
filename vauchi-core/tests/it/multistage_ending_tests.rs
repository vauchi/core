// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The ending after Finalized: a saved phone shows DONE, a saved phone that
//! read its peer's DONE shows BOTH for a short linger, and a phone that
//! reads BOTH stops at once. The 60 s grace stays the ceiling (design D6).

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::exchange::multistage::qr_codec::{
    self, StageQr, format_both_qr, format_done_qr, parse_qr,
};
use vauchi_core::exchange::multistage::session::{BOTH_LINGER, MultiStageSession};
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::monotonic::FakeMonotonicClock;

const TICK: Duration = Duration::from_millis(100);

struct Pair {
    alice: MultiStageSession,
    bob: MultiStageSession,
    clock: Arc<FakeMonotonicClock>,
}

fn pair() -> Pair {
    let clock = Arc::new(FakeMonotonicClock::new());
    Pair {
        alice: MultiStageSession::new(b"Alice (iPhone)".to_vec()).with_monotonic(clock.clone()),
        bob: MultiStageSession::new(b"Bob (Pixel)".to_vec()).with_monotonic(clock.clone()),
        clock,
    }
}

fn is_ending(frame: &str) -> bool {
    ["FIN3", "DON3", "BTH3"]
        .iter()
        .any(|p| frame.starts_with(p))
}

/// What a camera hands its session for a frame, if anything.
type Reader = fn(&str) -> Option<String>;

fn reads_everything(frame: &str) -> Option<String> {
    Some(frame.to_owned())
}

fn reads_no_ending(frame: &str) -> Option<String> {
    (!is_ending(frame)).then(|| frame.to_owned())
}

/// A DONE is read as the plain final frame it carries, as if caught just
/// before its sender saved; a BOTH is not read at all. Lets both sessions
/// save without either learning that the other has.
fn reads_final_but_not_done(frame: &str) -> Option<String> {
    match frame.get(..4) {
        Some("BTH3") => None,
        Some("DON3") => Some(format!("FIN3{}", &frame[4..])),
        _ => Some(frame.to_owned()),
    }
}

/// Each tick both phones show a frame and each reads the other's through
/// its reader, until `done` holds or the ticks run out.
fn run(p: &mut Pair, alice_reads: Reader, bob_reads: Reader, done: impl Fn(&Pair) -> bool) {
    for _ in 0..2000 {
        let aq = p.alice.get_display_qr();
        let bq = p.bob.get_display_qr();
        if let Some(seen) = aq.and_then(|q| bob_reads(&q.data)) {
            p.bob.process_scanned_qr(&seen);
        }
        if let Some(seen) = bq.and_then(|q| alice_reads(&q.data)) {
            p.alice.process_scanned_qr(&seen);
        }
        p.clock.advance(TICK);
        if done(p) {
            return;
        }
    }
    panic!("the exchange never reached the staged point");
}

/// Alice has saved; Bob has never read any of her ending frames, so he has
/// not.
fn alice_saved_bob_not() -> Pair {
    let mut p = pair();
    run(&mut p, reads_everything, reads_no_ending, |p| {
        p.alice.get_state() == ProtocolState::Finalized
    });
    assert_ne!(p.bob.get_state(), ProtocolState::Finalized);
    p
}

/// Both have saved, and neither has read the other's DONE yet.
fn both_saved() -> Pair {
    let mut p = pair();
    run(
        &mut p,
        reads_final_but_not_done,
        reads_final_but_not_done,
        |p| {
            p.alice.get_state() == ProtocolState::Finalized
                && p.bob.get_state() == ProtocolState::Finalized
        },
    );
    p
}

fn next_frame(session: &mut MultiStageSession) -> String {
    session
        .get_display_qr()
        .expect("a saved session still shows a frame")
        .data
}

/// The next frame that is not a DATA frame a saved phone interleaves for a
/// peer still missing chunks.
fn next_ending_frame(session: &mut MultiStageSession) -> String {
    for _ in 0..50 {
        let frame = next_frame(session);
        if !frame.starts_with("DAT3") {
            return frame;
        }
    }
    panic!("only DATA frames");
}

// @internal
#[test]
fn a_done_frame_carries_the_final_frame_under_its_own_prefix() {
    let sid = [7u8; 16];
    let reveal_key = [9u8; 32];
    let tag = [3u8; 32];
    let frame = format_done_qr(&sid, &reveal_key, &tag);

    assert!(frame.starts_with("DON3"));
    assert_eq!(
        frame.len(),
        qr_codec::format_final_qr(&sid, &reveal_key, &tag).len()
    );
    assert_eq!(
        parse_qr(&frame).expect("a done frame parses"),
        StageQr::Done {
            session_id: sid,
            reveal_key,
            tag,
        }
    );
}

// @internal
#[test]
fn a_both_frame_carries_only_the_session_id() {
    let sid = [5u8; 16];
    let frame = format_both_qr(&sid);

    assert!(frame.starts_with("BTH3"));
    assert_eq!(frame.len(), 4 + 8 + 24, "prefix, header, session id");
    assert_eq!(
        parse_qr(&frame).expect("a both frame parses"),
        StageQr::Both { session_id: sid }
    );
}

// @internal
#[test]
fn a_saved_session_shows_done_in_place_of_the_final_frame() {
    let mut p = alice_saved_bob_not();
    for _ in 0..20 {
        let frame = next_ending_frame(&mut p.alice);
        assert!(
            frame.starts_with("DON3"),
            "saved Alice showed {}",
            &frame[..4]
        );
    }
}

// @internal
#[test]
fn a_peer_that_has_not_saved_saves_on_reading_done() {
    let mut p = alice_saved_bob_not();
    let done = next_ending_frame(&mut p.alice);

    p.bob.process_scanned_qr(&done);

    assert_eq!(p.bob.get_state(), ProtocolState::Finalized);
}

// @internal
#[test]
fn a_saved_session_that_reads_the_peers_done_shows_both() {
    let mut p = both_saved();
    let alice_done = next_ending_frame(&mut p.alice);

    p.bob.process_scanned_qr(&alice_done);

    let frame = next_ending_frame(&mut p.bob);
    assert!(frame.starts_with("BTH3"), "Bob showed {}", &frame[..4]);
}

// @internal
#[test]
fn a_session_that_reads_both_stops_at_once() {
    let mut p = both_saved();
    let alice_done = next_ending_frame(&mut p.alice);
    p.bob.process_scanned_qr(&alice_done);
    let bob_both = next_ending_frame(&mut p.bob);

    p.alice.process_scanned_qr(&bob_both);

    assert_eq!(p.alice.get_display_qr().map(|q| q.data), None);
}

// @internal
#[test]
fn the_session_showing_both_stops_after_the_linger_and_not_before() {
    let mut p = both_saved();
    let alice_done = next_ending_frame(&mut p.alice);
    p.bob.process_scanned_qr(&alice_done);
    next_ending_frame(&mut p.bob);

    p.clock.advance(BOTH_LINGER - TICK);
    assert!(
        p.bob.get_display_qr().is_some(),
        "Bob stopped before the linger ran out"
    );

    p.clock.advance(TICK);
    assert_eq!(p.bob.get_display_qr().map(|q| q.data), None);
}

// @internal
#[test]
fn without_done_a_saved_session_keeps_showing_until_the_grace_ends() {
    let mut p = alice_saved_bob_not();

    p.clock.advance(Duration::from_secs(59));
    assert!(p.alice.get_display_qr().is_some());

    p.clock.advance(Duration::from_secs(2));
    assert_eq!(p.alice.get_display_qr().map(|q| q.data), None);
}

// @internal
#[test]
fn both_read_before_saving_does_not_stop_the_session() {
    let mut p = alice_saved_bob_not();
    let bob_sid = parse_qr(&next_frame(&mut p.bob))
        .expect("bob's frame parses")
        .session_id()
        .to_owned();
    let bob_state = p.bob.get_state();

    p.bob.process_scanned_qr(&format_both_qr(&bob_sid));
    let alice_sid = parse_qr(&next_ending_frame(&mut p.alice))
        .expect("alice's frame parses")
        .session_id()
        .to_owned();
    p.bob.process_scanned_qr(&format_both_qr(&alice_sid));

    assert_eq!(p.bob.get_state(), bob_state);
    assert!(p.bob.get_display_qr().is_some());
}

// @internal
#[test]
fn done_and_both_from_another_session_change_nothing() {
    let mut p = both_saved();
    let stranger = [0xEEu8; 16];

    p.alice
        .process_scanned_qr(&format_done_qr(&stranger, &[1u8; 32], &[2u8; 32]));
    p.alice.process_scanned_qr(&format_both_qr(&stranger));

    let frame = next_ending_frame(&mut p.alice);
    assert!(
        frame.starts_with("DON3"),
        "a stranger's DONE moved Alice to {}",
        &frame[..4]
    );
}

// @internal
#[test]
fn a_saved_session_is_confirmed_once_it_reads_done_or_both() {
    let mut p = alice_saved_bob_not();
    assert_eq!(
        p.alice.peer_confirmed(),
        false,
        "saved, but nothing from Bob says he has"
    );

    let alice_done = next_ending_frame(&mut p.alice);
    p.bob.process_scanned_qr(&alice_done);
    assert_eq!(p.bob.get_state(), ProtocolState::Finalized);
    assert_eq!(p.bob.peer_confirmed(), true, "Bob read Alice's DONE");

    let bob_both = next_ending_frame(&mut p.bob);
    p.alice.process_scanned_qr(&bob_both);
    assert_eq!(p.alice.peer_confirmed(), true, "Alice read Bob's BOTH");
}
