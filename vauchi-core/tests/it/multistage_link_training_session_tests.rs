// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Link training as seen through a session: the layout of each displayed
//! frame, and two sessions finding layouts each other's camera can read.

use std::sync::Arc;
use std::time::Duration;

use vauchi_core::exchange::multistage::link_trainer::{ECHO_FRESH, SWEEP_DWELL_MS};
use vauchi_core::exchange::multistage::qr_codec::parse_frame;
use vauchi_core::exchange::multistage::session::MultiStageSession;
use vauchi_core::exchange::multistage::training_header::LAYOUT_COUNT;
use vauchi_core::exchange::multistage::types::{ProtocolState, QrPayload};
use vauchi_core::monotonic::FakeMonotonicClock;

fn card(tag: u8) -> Vec<u8> {
    vec![tag; 150]
}

/// A camera that reads the peer's code only at some layouts.
struct ReadsLayouts(&'static [u8]);

impl ReadsLayouts {
    fn reads(&self, frame: &QrPayload) -> bool {
        self.0.contains(&frame.layout)
    }
}

const EVERY_LAYOUT: ReadsLayouts = ReadsLayouts(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]);

struct Outcome {
    finalized_at_tick: Option<usize>,
    alice_layouts: Vec<u8>,
    bob_layouts: Vec<u8>,
}

/// Two sessions on one clock. Each tick both show a frame, each camera reads
/// it if it can, and the clock moves on by the sweep dwell.
fn exchange(
    alice_camera: &ReadsLayouts,
    bob_camera: &ReadsLayouts,
    ticks: usize,
) -> (Outcome, MultiStageSession, MultiStageSession) {
    let clock = Arc::new(FakeMonotonicClock::new());
    let mut alice = MultiStageSession::new(card(0xA1)).with_monotonic(clock.clone());
    let mut bob = MultiStageSession::new(card(0xB2)).with_monotonic(clock.clone());
    let mut outcome = Outcome {
        finalized_at_tick: None,
        alice_layouts: Vec::new(),
        bob_layouts: Vec::new(),
    };

    for tick in 0..ticks {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        if let Some(aq) = &aq {
            outcome.alice_layouts.push(aq.layout);
            if bob_camera.reads(aq) {
                bob.process_scanned_qr(&aq.data);
            }
        }
        if let Some(bq) = &bq {
            outcome.bob_layouts.push(bq.layout);
            if alice_camera.reads(bq) {
                alice.process_scanned_qr(&bq.data);
            }
        }
        if outcome.finalized_at_tick.is_none()
            && matches!(alice.get_state(), ProtocolState::Finalized)
            && matches!(bob.get_state(), ProtocolState::Finalized)
        {
            outcome.finalized_at_tick = Some(tick);
        }
        clock.advance(Duration::from_millis(u64::from(SWEEP_DWELL_MS)));
    }
    (outcome, alice, bob)
}

// @internal
#[test]
fn each_frame_names_the_layout_it_is_drawn_at() {
    let mut session = MultiStageSession::new(card(0xA1));

    for _ in 0..20 {
        let frame = session.get_display_qr().expect("advertises");
        let header = parse_frame(&frame.data).expect("own frame parses").header;
        assert_eq!(header.layout(), frame.layout);
    }
}

// @internal
#[test]
fn a_session_with_no_peer_sweeps_its_opening_frame_through_every_layout() {
    let mut session = MultiStageSession::new(card(0xA1));

    let frames: Vec<QrPayload> = (0..usize::from(LAYOUT_COUNT))
        .map(|_| session.get_display_qr().expect("advertises"))
        .collect();

    let layouts: Vec<u8> = frames.iter().map(|f| f.layout).collect();
    assert_eq!(layouts, (0..LAYOUT_COUNT).collect::<Vec<u8>>());
    for frame in &frames {
        // The sweep dwell with its ±20 % jitter.
        assert!(
            (80..=120).contains(&frame.display_duration_ms),
            "sweep frame shown for {} ms",
            frame.display_duration_ms
        );
    }
}

// @internal
#[test]
fn a_session_starts_its_sweep_at_the_layout_it_is_given() {
    let mut session = MultiStageSession::new(card(0xA1)).with_start_layout(9);

    let first = session.get_display_qr().expect("advertises");

    assert_eq!(first.layout, 9);
}

// @internal
#[test]
fn a_session_drops_its_own_frames_and_counts_them() {
    let mut alice = MultiStageSession::new(card(0xA1));
    let own_opening = alice.get_display_qr().expect("advertises");

    for _ in 0..3 {
        alice.process_scanned_qr(&own_opening.data);
    }

    assert_eq!(alice.get_state(), ProtocolState::Advertising);
    assert_eq!(alice.peer_session_id(), None);
    assert_eq!(alice.own_frames_seen(), 3);
    // Seeing itself is no echo: the sweep goes on.
    let next = alice.get_display_qr().expect("advertises");
    assert_eq!(next.layout, 1);
    let header = parse_frame(&next.data).unwrap().header;
    assert_eq!(header.total_reads(), 0);
}

// @internal
#[test]
fn a_session_drops_its_own_data_frames_mid_transfer() {
    let (_, mut alice, _bob) = exchange(&EVERY_LAYOUT, &EVERY_LAYOUT, 2);
    assert!(
        matches!(alice.get_state(), ProtocolState::Transferring { .. }),
        "precondition: transferring, got {:?}",
        alice.get_state()
    );
    let state_before = alice.get_state();
    let seen_before = alice.own_frames_seen();

    let mut fed = 0;
    for _ in 0..8 {
        let own = alice.get_display_qr().expect("shows a frame");
        alice.process_scanned_qr(&own.data);
        fed += 1;
    }

    assert_eq!(alice.get_state(), state_before);
    assert_eq!(alice.own_frames_seen(), seen_before + fed);
}

// @internal
#[test]
fn cameras_that_read_every_layout_complete_at_full_size() {
    let (outcome, _, _) = exchange(&EVERY_LAYOUT, &EVERY_LAYOUT, 400);

    assert!(outcome.finalized_at_tick.is_some(), "did not finalize");
    // The first frame is read at full size, the second is already on its
    // way as the next sweep step, and the echo settles both from the third.
    assert_eq!(outcome.alice_layouts[..3], [0, 1, 0]);
    assert!(outcome.alice_layouts[3..].iter().all(|l| *l == 0));
    assert!(outcome.bob_layouts[3..].iter().all(|l| *l == 0));
}

// @internal
#[test]
fn two_sessions_settle_on_layouts_each_camera_reads_and_complete() {
    // Alice's camera sees Bob's code only at layout 7; Bob's sees Alice's
    // at 2 and 9.
    let alice_camera = ReadsLayouts(&[7]);
    let bob_camera = ReadsLayouts(&[2, 9]);

    let (outcome, _, _) = exchange(&alice_camera, &bob_camera, 1500);

    let finalized = outcome.finalized_at_tick.expect("did not finalize");
    // 18–32 ticks over 30 runs when written; a sweep is 14.
    assert!(finalized < 150, "took {finalized} ticks");
    // The frames that completed the exchange were drawn where the other
    // camera reads.
    assert!(
        [2, 9].contains(&outcome.alice_layouts[finalized]),
        "alice finished at layout {}",
        outcome.alice_layouts[finalized]
    );
    assert_eq!(outcome.bob_layouts[finalized], 7);
}

// @internal
#[test]
fn a_finalized_session_keeps_its_code_where_it_was() {
    let alice_camera = ReadsLayouts(&[7]);
    let bob_camera = ReadsLayouts(&[2]);
    let (outcome, _, _) = exchange(&alice_camera, &bob_camera, 1500);
    let finalized = outcome.finalized_at_tick.expect("did not finalize");

    // Long past the point an echo would have gone stale.
    let stale_after = usize::try_from(ECHO_FRESH.as_millis() / u128::from(SWEEP_DWELL_MS)).unwrap();
    let late = finalized + 3 * stale_after;
    assert!(late + 20 < outcome.alice_layouts.len(), "run long enough");

    assert!(
        outcome.alice_layouts[late..late + 20]
            .iter()
            .all(|l| *l == 2)
    );
    assert!(outcome.bob_layouts[late..late + 20].iter().all(|l| *l == 7));
}

// @internal
#[test]
fn cameras_that_read_nothing_leave_both_sessions_sweeping() {
    let blind = ReadsLayouts(&[]);

    let (outcome, alice, bob) = exchange(&blind, &blind, 200);

    assert_eq!(outcome.finalized_at_tick, None);
    assert_eq!(alice.get_state(), ProtocolState::Advertising);
    assert_eq!(bob.get_state(), ProtocolState::Advertising);
    let expected: Vec<u8> = (0..200u32)
        .map(|i| u8::try_from(i % u32::from(LAYOUT_COUNT)).unwrap())
        .collect();
    assert_eq!(outcome.alice_layouts, expected);
}

// @internal
#[test]
fn a_one_way_link_does_not_settle_the_side_that_is_not_read() {
    // Alice reads Bob everywhere; Bob never reads Alice.
    let blind = ReadsLayouts(&[]);

    let (outcome, _, _) = exchange(&EVERY_LAYOUT, &blind, 100);

    assert_eq!(outcome.finalized_at_tick, None);
    // Alice gets no echo, so she keeps sweeping.
    let last: Vec<u8> = outcome.alice_layouts[86..100].to_vec();
    let mut sorted = last.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, (0..LAYOUT_COUNT).collect::<Vec<u8>>(), "{last:?}");
}

// @internal
#[test]
fn a_bystanders_frames_do_not_train_a_bound_session() {
    let (_, mut alice, _bob) = exchange(&EVERY_LAYOUT, &EVERY_LAYOUT, 2);
    assert!(alice.peer_session_id().is_some(), "precondition: bound");
    let mut bystander = MultiStageSession::new(card(0xCC)).with_start_layout(12);
    let foreign = bystander.get_display_qr().expect("advertises");
    assert_eq!(foreign.layout, 12);
    let reads_before = parse_frame(&alice.get_display_qr().unwrap().data)
        .unwrap()
        .header
        .total_reads();

    for _ in 0..5 {
        alice.process_scanned_qr(&foreign.data);
    }

    let header = parse_frame(&alice.get_display_qr().unwrap().data)
        .unwrap()
        .header;
    assert_eq!(header.total_reads(), reads_before);
    assert!(header.echo().all(|reads| reads.layout != 12));
}

/// Alice's DATA frames after she read Bob's opening frame carrying a header
/// that reports `peer_reads` reads in his window.
fn data_frame_lengths_for_a_peer_reporting(peer_reads: u8) -> Vec<usize> {
    use vauchi_core::exchange::multistage::qr_codec::with_header;
    use vauchi_core::exchange::multistage::training_header::TrainingHeader;

    // 400 bytes: enough chunks that most frames carry a full one.
    let mut alice = MultiStageSession::new(vec![0xA1; 400]);
    let mut bob = MultiStageSession::new(card(0xB2));
    alice.get_display_qr().expect("advertises");
    let opening = bob.get_display_qr().expect("advertises");
    let header = TrainingHeader::new(0, &[], peer_reads).expect("within range");
    alice.process_scanned_qr(&with_header(&opening.data, &header).expect("takes a header"));

    (0..80)
        .filter_map(|_| alice.get_display_qr())
        .filter(|frame| frame.data.starts_with("DAT3"))
        .map(|frame| frame.data.len())
        .collect()
}

// @internal
#[test]
fn a_link_with_no_read_rate_yet_sends_chunks_that_fit_qr_version_6() {
    let lengths = data_frame_lengths_for_a_peer_reporting(0);

    assert!(!lengths.is_empty(), "no DATA frames shown");
    let longest = *lengths.iter().max().unwrap();
    // 11 characters short of the version's 154: the ACK field is empty
    // until the peer's chunk count is known, and may grow to 5.
    assert_eq!(longest, 143);
}

// @internal
#[test]
fn data_frames_stay_in_qr_version_6_while_acking_a_24_chunk_peer() {
    use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_qr};

    let mut alice = MultiStageSession::new(vec![0xA1; 400]);
    // 24 chunks of 34 bytes once sealed (776 + 24 + 16 = 816 bytes).
    let mut bob = MultiStageSession::new(vec![0xB2; 776]);
    let a_opening = alice.get_display_qr().expect("advertises");
    let b_opening = bob.get_display_qr().expect("advertises");
    alice.process_scanned_qr(&b_opening.data);
    bob.process_scanned_qr(&a_opening.data);
    // Alice learns Bob's chunk count from his first DATA frame.
    let bob_data = (0..40)
        .filter_map(|_| bob.get_display_qr())
        .find(|frame| frame.data.starts_with("DAT3"))
        .expect("bob shows DATA");
    let Ok(StageQr::Data { chunk_total, .. }) = parse_qr(&bob_data.data) else {
        panic!("not a DATA frame");
    };
    assert_eq!(chunk_total, 24, "precondition: a 24-chunk peer");
    alice.process_scanned_qr(&bob_data.data);

    let longest = (0..80)
        .filter_map(|_| alice.get_display_qr())
        .filter(|frame| frame.data.starts_with("DAT3"))
        .map(|frame| frame.data.len())
        .max()
        .expect("alice shows DATA");

    assert_eq!(longest, 148, "a full chunk with a 3-byte ACK");
}

// @internal
#[test]
fn a_peer_that_reports_reading_fast_gets_denser_chunks() {
    use vauchi_core::exchange::multistage::link_trainer::{
        STEP_UP_TO_V7_READS, STEP_UP_TO_V8_READS,
    };

    let just_below = data_frame_lengths_for_a_peer_reporting(STEP_UP_TO_V7_READS - 1);
    let version_7 = data_frame_lengths_for_a_peer_reporting(STEP_UP_TO_V7_READS);
    let version_8 = data_frame_lengths_for_a_peer_reporting(STEP_UP_TO_V8_READS);

    // Each 11 characters short of what its QR version holds (154, 178,
    // 221), kept for the ACK field.
    assert_eq!(*just_below.iter().max().unwrap(), 143);
    assert_eq!(*version_7.iter().max().unwrap(), 167);
    assert_eq!(*version_8.iter().max().unwrap(), 209);
}

// @internal
#[test]
fn the_chunk_size_does_not_change_once_data_has_started() {
    let (_, mut alice, mut bob) = exchange(&EVERY_LAYOUT, &EVERY_LAYOUT, 2);
    assert!(matches!(
        alice.get_state(),
        ProtocolState::Transferring { .. }
    ));
    let totals_seen = |session: &mut MultiStageSession| -> Vec<u16> {
        use vauchi_core::exchange::multistage::qr_codec::{StageQr, parse_qr};
        let mut totals: Vec<u16> = (0..40)
            .filter_map(|_| session.get_display_qr())
            .filter_map(|frame| match parse_qr(&frame.data) {
                Ok(StageQr::Data { chunk_total, .. }) => Some(chunk_total),
                _ => None,
            })
            .collect();
        totals.dedup();
        totals
    };
    let before = totals_seen(&mut alice);

    // Bob now reports a saturated read rate on every frame.
    for _ in 0..60 {
        if let Some(frame) = bob.get_display_qr() {
            for _ in 0..3 {
                alice.process_scanned_qr(&frame.data);
            }
        }
        let _ = alice
            .get_display_qr()
            .map(|f| bob.process_scanned_qr(&f.data));
        if !matches!(alice.get_state(), ProtocolState::Transferring { .. }) {
            break;
        }
    }

    assert_eq!(before.len(), 1, "one chunk count per transfer: {before:?}");
    let after = totals_seen(&mut alice);
    assert!(
        after.is_empty() || after == before,
        "chunk count changed mid-transfer: {before:?} then {after:?}"
    );
}

// @internal
#[test]
fn every_frame_a_session_shows_fits_37_modules_at_the_level_it_names() {
    let mut alice = MultiStageSession::new(vec![0xA1; 400]);
    let mut bob = MultiStageSession::new(vec![0xB2; 400]);
    let mut seen = std::collections::BTreeSet::new();

    for _ in 0..400 {
        let aq = alice.get_display_qr();
        let bq = bob.get_display_qr();
        for frame in [&aq, &bq].into_iter().flatten() {
            assert_eq!(frame.error_correction, "L", "{}", &frame.data[..4]);
            let code = qrcode::QrCode::with_error_correction_level(&frame.data, qrcode::EcLevel::L)
                .expect("frame encodes");
            assert_eq!(
                code.width(),
                37,
                "a {}-char {} frame: {}",
                frame.data.len(),
                &frame.data[..4],
                frame.data
            );
            seen.insert(frame.data[..4].to_string());
        }
        if let Some(aq) = &aq {
            bob.process_scanned_qr(&aq.data);
        }
        if let Some(bq) = &bq {
            alice.process_scanned_qr(&bq.data);
        }
    }

    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        ["DAT3", "FIN3", "INI3"],
        "the run showed every frame type"
    );
}
