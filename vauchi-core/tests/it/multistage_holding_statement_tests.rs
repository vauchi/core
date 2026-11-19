// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a Hover confirm and done say about the contact list
//! (vauchi/private#1285).
//!
//! A confirm states whether its sender already holds the reader as a
//! contact; a done adds how the exchange ended there. The statement is the
//! tag's label, so only the peer of this session can read it, and a phone
//! makes it only after the peer's signed payload verified: a stranger who
//! names an identity it cannot sign for learns nothing about who holds it.

use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use vauchi_core::exchange::multistage::confirmation::{
    self, Ending, Holding, Side, TagKind, TagLabel,
};
use vauchi_core::exchange::multistage::qr_codec::{
    self, QrCodecError, StageQr, format_both_qr, format_done_qr, format_final_qr, parse_qr,
};
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::types::ProtocolState;
use vauchi_core::monotonic::FakeMonotonicClock;
use vauchi_core::{ContactCard, Identity};

const KEY: [u8; 32] = [7u8; 32];

const EVERY_LABEL: [TagLabel; 7] = [
    TagLabel::Confirm(Holding::Peer),
    TagLabel::Confirm(Holding::Nothing),
    TagLabel::Done(Ending::Saved(Holding::Peer)),
    TagLabel::Done(Ending::Saved(Holding::Nothing)),
    TagLabel::Done(Ending::Reconnected),
    TagLabel::Done(Ending::Kept(Holding::Peer)),
    TagLabel::Done(Ending::Kept(Holding::Nothing)),
];

fn side<'a>(payload: &'a [u8], ids: &'a ([u8; 16], [u8; 32], [u8; 32])) -> Side<'a> {
    Side {
        session_id: &ids.0,
        ephemeral: &ids.1,
        commitment_hash: &ids.2,
        payload,
    }
}

fn ids(seed: u8) -> ([u8; 16], [u8; 32], [u8; 32]) {
    (
        [seed; 16],
        [seed.wrapping_add(1); 32],
        [seed.wrapping_add(2); 32],
    )
}

fn tag_under(label: TagLabel) -> [u8; 32] {
    let (a, b) = (ids(1), ids(9));
    confirmation::tag(&KEY, label, &side(b"alice", &a), &side(b"bob", &b))
}

fn read_as(kind: TagKind, tag: &[u8; 32]) -> Option<TagLabel> {
    let (a, b) = (ids(1), ids(9));
    confirmation::read(&KEY, kind, tag, &side(b"alice", &a), &side(b"bob", &b))
}

// @internal
#[test]
fn every_label_gives_its_own_tag() {
    for (i, first) in EVERY_LABEL.iter().enumerate() {
        for second in &EVERY_LABEL[i + 1..] {
            assert_ne!(
                tag_under(*first),
                tag_under(*second),
                "{first:?} and {second:?} share a tag"
            );
        }
    }
}

// @internal
#[test]
fn a_tag_reads_back_as_the_label_it_was_made_under_and_as_no_other() {
    for label in EVERY_LABEL {
        let kind = match label {
            TagLabel::Confirm(_) => TagKind::Confirm,
            TagLabel::Done(_) => TagKind::Done,
        };
        assert_eq!(read_as(kind, &tag_under(label)), Some(label));
    }
}

// @internal
#[test]
fn a_done_tag_does_not_read_as_a_confirm_nor_a_confirm_tag_as_a_done() {
    for label in EVERY_LABEL {
        let other_kind = match label {
            TagLabel::Confirm(_) => TagKind::Done,
            TagLabel::Done(_) => TagKind::Confirm,
        };
        assert_eq!(
            read_as(other_kind, &tag_under(label)),
            None,
            "{label:?} read as the other kind of frame"
        );
    }
}

// @internal
#[test]
fn a_tag_from_another_key_or_another_binding_reads_as_nothing() {
    let (a, b, c) = (ids(1), ids(9), ids(40));
    let label = TagLabel::Confirm(Holding::Peer);
    let tag = tag_under(label);

    let other_key = confirmation::read(
        &[8u8; 32],
        TagKind::Confirm,
        &tag,
        &side(b"alice", &a),
        &side(b"bob", &b),
    );
    let other_receiver = confirmation::read(
        &KEY,
        TagKind::Confirm,
        &tag,
        &side(b"alice", &a),
        &side(b"bob", &c),
    );
    let swapped_direction = confirmation::read(
        &KEY,
        TagKind::Confirm,
        &tag,
        &side(b"bob", &b),
        &side(b"alice", &a),
    );
    let other_payload = confirmation::read(
        &KEY,
        TagKind::Confirm,
        &tag,
        &side(b"mallory", &a),
        &side(b"bob", &b),
    );

    assert_eq!(
        (other_key, other_receiver, swapped_direction, other_payload),
        (None, None, None, None)
    );
}

proptest! {
    // @internal
    #[test]
    fn an_arbitrary_tag_reads_as_no_label(tag in any::<[u8; 32]>()) {
        prop_assume!(!EVERY_LABEL.iter().any(|label| tag_under(*label) == tag));
        prop_assert_eq!(read_as(TagKind::Confirm, &tag), None);
        prop_assert_eq!(read_as(TagKind::Done, &tag), None);
    }
}

// @internal
#[test]
fn the_ending_frames_carry_the_version_five_prefixes() {
    let (sid, reveal_key, tag) = ([3u8; 16], [4u8; 32], [5u8; 32]);

    assert_eq!(&format_final_qr(&sid, &reveal_key, &tag)[..4], "CNF5");
    assert_eq!(&format_done_qr(&sid, &reveal_key, &tag)[..4], "DON5");
    assert_eq!(&format_both_qr(&sid, &reveal_key, &tag)[..4], "BTH5");
}

// @internal
#[test]
fn an_ending_frame_does_not_show_which_statement_it_carries() {
    let (sid, reveal_key) = ([3u8; 16], [4u8; 32]);
    let holds = format_final_qr(
        &sid,
        &reveal_key,
        &tag_under(TagLabel::Confirm(Holding::Peer)),
    );
    let holds_nothing = format_final_qr(
        &sid,
        &reveal_key,
        &tag_under(TagLabel::Confirm(Holding::Nothing)),
    );

    let tag_starts_at = holds.len() - 48;
    assert_eq!(holds.len(), holds_nothing.len());
    assert_eq!(&holds[..tag_starts_at], &holds_nothing[..tag_starts_at]);
    assert_ne!(&holds[tag_starts_at..], &holds_nothing[tag_starts_at..]);
}

// @internal
#[test]
fn the_version_four_ending_frames_are_an_old_format() {
    for prefix in ["CNF4", "DON4", "BTH4"] {
        let mut frame = format_final_qr(&[3u8; 16], &[4u8; 32], &[5u8; 32]);
        frame.replace_range(..4, prefix);

        assert!(
            matches!(parse_qr(&frame), Err(QrCodecError::OldFormat)),
            "{prefix} was not reported as an older version"
        );
    }
}

struct Pair {
    alice: MultiStageSession,
    bob: MultiStageSession,
    bob_identity: Identity,
    clock: Arc<FakeMonotonicClock>,
}

fn pair() -> Pair {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);
    let clock = Arc::new(FakeMonotonicClock::new());
    Pair {
        alice: MultiStageSession::for_identity(&alice_identity, &ContactCard::new("Alice"), None)
            .with_monotonic(clock.clone()),
        bob: MultiStageSession::for_identity(&bob_identity, &ContactCard::new("Bob"), None)
            .with_monotonic(clock.clone()),
        bob_identity,
        clock,
    }
}

fn is_ending(frame: &str) -> bool {
    ["CNF5", "DON5", "BTH5"].contains(&&frame[..4])
}

/// Each phone reads every frame the other shows, and states what `holds`
/// says as soon as its session asks.
fn run(
    p: &mut Pair,
    alice_holds: Option<Holding>,
    bob_holds: Option<Holding>,
    done: impl Fn(&Pair) -> bool,
) {
    let (ai, bi) = (
        p.alice.get_display_qr().unwrap(),
        p.bob.get_display_qr().unwrap(),
    );
    p.alice.process_scanned_qr(&bi.data);
    p.bob.process_scanned_qr(&ai.data);
    for _ in 0..400 {
        for (session, holds) in [(&mut p.alice, alice_holds), (&mut p.bob, bob_holds)] {
            if let (Some(_), Some(holding)) = (session.peer_awaiting_statement(), holds) {
                assert!(session.state_holding(holding));
            }
        }
        if let Some(frame) = p.alice.get_display_qr() {
            p.bob.process_scanned_qr(&frame.data);
        }
        if let Some(frame) = p.bob.get_display_qr() {
            p.alice.process_scanned_qr(&frame.data);
        }
        if done(p) {
            return;
        }
    }
}

fn both_finalized(p: &Pair) -> bool {
    p.alice.get_state() == ProtocolState::Finalized && p.bob.get_state() == ProtocolState::Finalized
}

fn frames_shown(session: &mut MultiStageSession, count: usize) -> Vec<String> {
    (0..count)
        .filter_map(|_| session.get_display_qr().map(|frame| frame.data))
        .collect()
}

// @internal
#[test]
fn each_phone_learns_what_the_other_stated() {
    let mut p = pair();

    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );

    assert!(both_finalized(&p));
    assert_eq!(p.alice.peer_holding(), Some(Holding::Nothing));
    assert_eq!(p.bob.peer_holding(), Some(Holding::Peer));
}

// @internal
#[test]
fn a_phone_states_nothing_until_its_app_said_what_it_holds() {
    let mut p = pair();

    run(&mut p, None, Some(Holding::Nothing), |p| {
        p.alice.peer_awaiting_statement().is_some()
            && p.bob.get_state() == ProtocolState::Confirming
    });

    assert_eq!(
        p.alice.peer_awaiting_statement(),
        Some(*p.bob_identity.signing_public_key())
    );
    let shown = frames_shown(&mut p.alice, 30);
    assert!(!shown.is_empty());
    assert!(
        shown.iter().all(|frame| !is_ending(frame)),
        "a confirm left the phone before its app said what it holds"
    );
    assert_eq!(p.bob.get_state(), ProtocolState::Confirming);
    assert_eq!(p.bob.peer_holding(), None);
}

// @internal
#[test]
fn a_statement_made_before_the_peer_verified_is_refused() {
    let mut p = pair();
    let (ai, bi) = (
        p.alice.get_display_qr().unwrap(),
        p.bob.get_display_qr().unwrap(),
    );
    p.alice.process_scanned_qr(&bi.data);
    p.bob.process_scanned_qr(&ai.data);

    assert_eq!(p.alice.peer_awaiting_statement(), None);
    assert!(!p.alice.state_holding(Holding::Peer));

    run(&mut p, None, Some(Holding::Nothing), |p| {
        p.alice.peer_awaiting_statement().is_some()
    });
    assert!(
        frames_shown(&mut p.alice, 30)
            .iter()
            .all(|frame| !is_ending(frame)),
        "the refused statement was kept and used once the peer verified"
    );
}

// @internal
#[test]
fn a_peer_that_cannot_sign_for_the_identity_it_names_gets_no_statement() {
    let alice_identity = Identity::create("Alice", 0);
    let bob_identity = Identity::create("Bob", 0);
    let mut forged = vec![2u8];
    forged.extend_from_slice(bob_identity.signing_public_key());
    forged.extend_from_slice(&[0u8; 64]);
    forged.extend_from_slice(&serde_json::to_vec(&ContactCard::new("Bob")).unwrap());
    let mut p = Pair {
        alice: MultiStageSession::for_identity(&alice_identity, &ContactCard::new("Alice"), None),
        bob: MultiStageSession::new(forged),
        bob_identity,
        clock: Arc::new(FakeMonotonicClock::new()),
    };
    let mut shown_by_alice = Vec::new();

    let (ai, bi) = (
        p.alice.get_display_qr().unwrap(),
        p.bob.get_display_qr().unwrap(),
    );
    p.alice.process_scanned_qr(&bi.data);
    p.bob.process_scanned_qr(&ai.data);
    for _ in 0..400 {
        assert_eq!(p.alice.peer_awaiting_statement(), None);
        assert!(!p.alice.state_holding(Holding::Peer));
        if let Some(frame) = p.alice.get_display_qr() {
            p.bob.process_scanned_qr(&frame.data);
            shown_by_alice.push(frame.data);
        }
        if let Some(frame) = p.bob.get_display_qr() {
            p.alice.process_scanned_qr(&frame.data);
        }
        if matches!(p.alice.get_state(), ProtocolState::Failed(_)) {
            break;
        }
    }

    assert!(matches!(p.alice.get_state(), ProtocolState::Failed(_)));
    assert!(shown_by_alice.iter().all(|frame| !is_ending(frame)));
    assert_eq!(p.alice.verified_peer().map(|peer| peer.identity_pk), None);
}

// @internal
#[test]
fn a_statement_cannot_be_changed_once_made() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Nothing),
        Some(Holding::Nothing),
        both_finalized,
    );

    assert!(!p.alice.state_holding(Holding::Peer));

    assert_eq!(p.bob.peer_holding(), Some(Holding::Nothing));
}

fn done_tag_of(session: &mut MultiStageSession) -> Option<[u8; 32]> {
    frames_shown(session, 20)
        .iter()
        .find_map(|frame| match parse_qr(frame) {
            Ok(StageQr::Done { tag, .. }) => Some(tag),
            _ => None,
        })
}

fn deliver(from: &mut MultiStageSession, to: &mut MultiStageSession) {
    for frame in frames_shown(from, 20) {
        to.process_scanned_qr(&frame);
    }
}

// @internal
#[test]
fn a_saved_phone_says_saved_with_what_it_held() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Peer),
        both_finalized,
    );

    assert!(p.alice.mark_saved());
    deliver(&mut p.alice, &mut p.bob);

    assert_eq!(p.bob.peer_ending(), Some(Ending::Saved(Holding::Peer)));
    assert!(p.bob.peer_saved());
}

// @internal
#[test]
fn a_holder_whose_person_agreed_says_reconnected() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );

    assert!(p.alice.mark_reconnected());
    deliver(&mut p.alice, &mut p.bob);

    assert_eq!(p.bob.peer_ending(), Some(Ending::Reconnected));
    assert!(p.bob.peer_saved());
}

// @internal
#[test]
fn a_holder_whose_person_declined_says_kept_and_the_peer_does_not_count_it_as_saved() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );

    assert!(p.alice.mark_kept());
    deliver(&mut p.alice, &mut p.bob);

    assert_eq!(p.bob.peer_ending(), Some(Ending::Kept(Holding::Peer)));
    assert!(!p.bob.peer_saved());
    assert!(!p.bob.peer_confirmed());
}

// @internal
#[test]
fn only_a_holder_whose_peer_holds_nothing_can_say_reconnected() {
    for (alice_holds, bob_holds) in [
        (Holding::Nothing, Holding::Nothing),
        (Holding::Peer, Holding::Peer),
        (Holding::Nothing, Holding::Peer),
    ] {
        let mut p = pair();
        run(&mut p, Some(alice_holds), Some(bob_holds), both_finalized);

        assert!(
            !p.alice.mark_reconnected(),
            "reconnected was accepted with {alice_holds:?} here and {bob_holds:?} there"
        );
        assert_eq!(done_tag_of(&mut p.alice), None);
    }
}

// @internal
#[test]
fn an_ending_is_said_once() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );

    assert!(p.alice.mark_kept());
    assert!(!p.alice.mark_reconnected());
    assert!(!p.alice.mark_saved());
    deliver(&mut p.alice, &mut p.bob);

    assert_eq!(p.bob.peer_ending(), Some(Ending::Kept(Holding::Peer)));
}

// @internal
#[test]
fn a_phone_that_ended_with_kept_stops_on_its_peers_both_as_a_saved_one_does() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );
    assert!(p.alice.mark_kept());
    deliver(&mut p.alice, &mut p.bob);
    assert!(p.bob.mark_kept());

    let shown_by_bob = frames_shown(&mut p.bob, 5);
    for frame in &shown_by_bob {
        p.alice.process_scanned_qr(frame);
    }

    assert!(shown_by_bob.iter().all(|frame| frame.starts_with("BTH5")));
    assert_eq!(p.alice.peer_ending(), Some(Ending::Kept(Holding::Nothing)));
    assert_eq!(p.alice.get_display_qr().map(|frame| frame.data), None);
}

// @internal
#[test]
fn a_phone_whose_person_is_asked_keeps_its_confirm_up_past_the_usual_grace() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );

    p.clock.advance(Duration::from_secs(120));
    let shown_at_two_minutes = frames_shown(&mut p.alice, 8);
    p.clock.advance(Duration::from_secs(31));
    let shown_past_the_ceiling = p.alice.get_display_qr().map(|frame| frame.data);

    assert!(
        shown_at_two_minutes
            .iter()
            .any(|frame| frame.starts_with("CNF5"))
    );
    assert_eq!(shown_past_the_ceiling, None);
}

// @internal
#[test]
fn two_phones_that_agree_on_what_they_hold_stop_at_the_usual_grace() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Peer),
        both_finalized,
    );

    p.clock.advance(Duration::from_secs(61));

    assert_eq!(p.alice.get_display_qr().map(|frame| frame.data), None);
}

// @internal
#[test]
fn a_late_answer_gives_the_peer_a_full_grace_to_read_it() {
    let mut p = pair();
    run(
        &mut p,
        Some(Holding::Peer),
        Some(Holding::Nothing),
        both_finalized,
    );
    p.clock.advance(Duration::from_secs(100));

    assert!(p.alice.mark_reconnected());
    p.clock.advance(Duration::from_secs(59));
    let shown_within_the_grace = frames_shown(&mut p.alice, 8);
    p.clock.advance(Duration::from_secs(2));
    let shown_after_it = p.alice.get_display_qr().map(|frame| frame.data);

    assert!(
        shown_within_the_grace
            .iter()
            .any(|frame| frame.starts_with("DON5"))
    );
    assert_eq!(shown_after_it, None);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    // @internal
    #[test]
    fn an_ending_frame_with_an_arbitrary_tag_tells_a_waiting_phone_nothing(
        tag in any::<[u8; 32]>(),
        kind in 0u8..3,
    ) {
        let mut p = pair();
        run(&mut p, Some(Holding::Nothing), None, |p| {
            p.alice.get_state() == ProtocolState::Confirming
        });
        prop_assert_eq!(p.alice.get_state(), ProtocolState::Confirming);
        let bob_sid = p.bob.session_id();
        let reveal_key = frames_shown(&mut p.bob, 20)
            .iter()
            .find_map(|frame| match parse_qr(frame) {
                Ok(StageQr::Reveal { reveal_key, .. }) => Some(reveal_key),
                _ => None,
            })
            .expect("Bob shows his reveal while his app has not answered");

        let forged = match kind {
            0 => qr_codec::format_final_qr(&bob_sid, &reveal_key, &tag),
            1 => qr_codec::format_done_qr(&bob_sid, &reveal_key, &tag),
            _ => qr_codec::format_both_qr(&bob_sid, &reveal_key, &tag),
        };
        p.alice.process_scanned_qr(&forged);

        prop_assert_eq!(p.alice.get_state(), ProtocolState::Confirming);
        prop_assert_eq!(p.alice.peer_holding(), None);
        prop_assert_eq!(p.alice.peer_ending(), None);
    }
}
