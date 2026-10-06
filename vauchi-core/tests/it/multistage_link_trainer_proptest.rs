// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link trainer under random sequences of reads, frames, holds and
//! clock steps (CC-13, #450 plan 2.8). Each frame it draws must follow the
//! rules the trainer promises: a held trainer stays put, a fresh echo puts
//! the code where the peer reads it most, and a sweep walks every layout in
//! turn from the last one the peer echoed.

use std::time::{Duration, Instant};

use proptest::prelude::*;
use vauchi_core::exchange::multistage::link_trainer::*;
use vauchi_core::exchange::multistage::training_header::*;

#[derive(Debug, Clone)]
enum Step {
    Wait(u64),
    ReadPeerFrame(TrainingHeader),
    ShowFrame,
    Hold,
}

fn peer_header() -> impl Strategy<Value = TrainingHeader> {
    (
        0..LAYOUT_COUNT,
        prop::collection::btree_map(0..LAYOUT_COUNT, 1..=MAX_READ_COUNT, 0..=ECHO_SLOTS),
        0..=MAX_READ_COUNT,
    )
        .prop_map(|(layout, echo, total)| {
            let echo: Vec<LayoutReads> = echo
                .into_iter()
                .map(|(layout, count)| LayoutReads { layout, count })
                .collect();
            TrainingHeader::new(layout, &echo, total).expect("generated within range")
        })
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        3 => (0u64..2500).prop_map(Step::Wait),
        3 => peer_header().prop_map(Step::ReadPeerFrame),
        4 => Just(Step::ShowFrame),
        1 => Just(Step::Hold),
    ]
}

/// The layouts a peer's echo names as read most.
fn read_most(header: &TrainingHeader) -> Vec<u8> {
    let most = header.echo().map(|reads| reads.count).max();
    header
        .echo()
        .filter(|reads| Some(reads.count) == most)
        .map(|reads| reads.layout)
        .collect()
}

/// What the rules need to know about the past.
#[derive(Default)]
struct Seen {
    reads: Vec<Instant>,
    /// The last echo heard while adapting, and when.
    echo: Option<(Vec<u8>, Instant)>,
    held: bool,
    held_at: Option<u8>,
    shown: Option<u8>,
    /// Layout of the previous frame when it was a sweep frame and no echo
    /// has been heard since.
    sweeping_from: Option<u8>,
}

fn check_frame(seen: &mut Seen, header: &TrainingHeader, start: u8, now: Instant) {
    let layout = header.layout();
    let in_window = seen
        .reads
        .iter()
        .filter(|at| now.saturating_duration_since(**at) < READ_WINDOW)
        .count();
    assert_eq!(
        usize::from(header.total_reads()),
        in_window.min(usize::from(MAX_READ_COUNT)),
        "the header counts the peer frames read in the window"
    );
    assert!(header.echo().count() <= ECHO_SLOTS);

    let fresh = seen
        .echo
        .as_ref()
        .filter(|(_, at)| now.saturating_duration_since(*at) < ECHO_FRESH);
    if seen.held {
        let held_at = *seen.held_at.get_or_insert(layout);
        assert_eq!(layout, held_at, "a held trainer draws where it was");
    } else if let Some((most, _)) = fresh {
        assert!(
            most.contains(&layout),
            "a fresh echo puts the code at a layout the peer read most: {layout} not in {most:?}"
        );
        seen.sweeping_from = None;
    } else {
        let expected = match (seen.sweeping_from, &seen.echo) {
            (Some(previous), _) => vec![(previous + 1) % LAYOUT_COUNT],
            (None, Some((most, _))) => most.clone(),
            (None, None) => vec![start],
        };
        assert!(
            expected.contains(&layout),
            "a sweep frame at {layout}, expected one of {expected:?}"
        );
        seen.sweeping_from = Some(layout);
    }
    seen.shown = Some(layout);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    // @internal
    #[test]
    fn every_frame_follows_the_trainers_rules(
        start in 0u8..20,
        steps in prop::collection::vec(step(), 1..120),
    ) {
        let mut trainer = LinkTrainer::starting_at(start);
        let start = if start < LAYOUT_COUNT { start } else { 0 };
        let mut now = Instant::now();
        let mut seen = Seen::default();

        for step in steps {
            match step {
                Step::Wait(ms) => now += Duration::from_millis(ms),
                Step::ReadPeerFrame(header) => {
                    trainer.note_peer_frame(&header, now);
                    seen.reads.push(now);
                    if !seen.held && header.echo().count() > 0 {
                        seen.echo = Some((read_most(&header), now));
                        seen.sweeping_from = None;
                    }
                }
                Step::ShowFrame => {
                    let header = trainer.next_frame(now);
                    check_frame(&mut seen, &header, start, now);
                }
                Step::Hold => {
                    trainer.hold();
                    if !seen.held {
                        seen.held = true;
                        seen.held_at = seen.shown;
                    }
                }
            }
            prop_assert_eq!(
                trainer.last_good_layout().is_some(),
                seen.echo.is_some(),
                "a last good layout exists once the peer has echoed one"
            );
        }
    }
}
