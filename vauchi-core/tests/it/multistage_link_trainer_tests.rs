// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The link trainer as a state machine: which layout the next frame is drawn
//! at, and what the next header tells the peer.

use std::time::{Duration, Instant};

use vauchi_core::exchange::multistage::link_trainer::*;
use vauchi_core::exchange::multistage::training_header::*;

fn peer_frame(layout: u8, echo: &[(u8, u8)]) -> TrainingHeader {
    let echo: Vec<LayoutReads> = echo
        .iter()
        .map(|&(layout, count)| LayoutReads { layout, count })
        .collect();
    TrainingHeader::new(layout, &echo, 0).expect("header within range")
}

fn layouts_of_next_frames(trainer: &mut LinkTrainer, now: Instant, frames: usize) -> Vec<u8> {
    (0..frames)
        .map(|_| trainer.next_frame(now).layout())
        .collect()
}

// @internal
#[test]
fn with_no_echo_the_sweep_visits_every_layout_starting_at_the_last_good_one() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(5);

    let shown = layouts_of_next_frames(&mut trainer, now, 14);

    assert_eq!(shown, vec![5, 6, 7, 8, 9, 10, 11, 12, 13, 0, 1, 2, 3, 4]);
    assert!(trainer.is_sweeping(now));
}

// @internal
#[test]
fn a_start_layout_outside_the_set_starts_at_full_size() {
    let mut trainer = LinkTrainer::starting_at(200);

    assert_eq!(trainer.next_frame(Instant::now()).layout(), 0);
}

// @internal
#[test]
fn an_echo_settles_on_the_layout_the_peer_read_most() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);

    trainer.note_peer_frame(&peer_frame(2, &[(3, 2), (7, 9), (1, 4)]), now);

    assert_eq!(layouts_of_next_frames(&mut trainer, now, 5), vec![7; 5]);
    assert!(!trainer.is_sweeping(now));
    assert_eq!(trainer.last_good_layout(), Some(7));
}

// @internal
#[test]
fn equal_counts_settle_on_the_larger_layout() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);

    trainer.note_peer_frame(&peer_frame(0, &[(9, 6), (4, 6)]), now);

    assert_eq!(trainer.next_frame(now).layout(), 4);
}

// @internal
#[test]
fn a_settled_layout_is_kept_while_the_peer_still_reads_it_as_well_as_any() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    trainer.note_peer_frame(&peer_frame(0, &[(9, 6)]), now);
    assert_eq!(trainer.next_frame(now).layout(), 9);

    // A larger layout ties: no reason to jump.
    trainer.note_peer_frame(&peer_frame(0, &[(4, 6), (9, 6)]), now);
    assert_eq!(trainer.next_frame(now).layout(), 9);

    // The peer now reads another one more often.
    trainer.note_peer_frame(&peer_frame(0, &[(4, 20), (9, 6)]), now);
    assert_eq!(trainer.next_frame(now).layout(), 4);
}

// @internal
#[test]
fn a_frame_without_an_echo_does_not_stop_the_sweep() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);

    trainer.note_peer_frame(&peer_frame(6, &[]), now);

    assert_eq!(layouts_of_next_frames(&mut trainer, now, 3), vec![0, 1, 2]);
    assert!(trainer.is_sweeping(now));
    assert_eq!(trainer.last_good_layout(), None);
}

// @internal
#[test]
fn the_header_echoes_the_three_peer_layouts_read_most() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    for (layout, times) in [(2u8, 3usize), (5, 7), (9, 1), (11, 4)] {
        for _ in 0..times {
            trainer.note_peer_frame(&peer_frame(layout, &[]), now);
        }
    }

    let header = trainer.next_frame(now);

    let echo: Vec<(u8, u8)> = header.echo().map(|r| (r.layout, r.count)).collect();
    assert_eq!(echo, vec![(5, 7), (11, 4), (2, 3)]);
    assert_eq!(header.total_reads(), 15);
}

// @internal
#[test]
fn read_counts_saturate_at_one_digit() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    for _ in 0..200 {
        trainer.note_peer_frame(&peer_frame(3, &[]), now);
    }

    let header = trainer.next_frame(now);

    let echo: Vec<(u8, u8)> = header.echo().map(|r| (r.layout, r.count)).collect();
    assert_eq!(echo, vec![(3, MAX_READ_COUNT)]);
    assert_eq!(header.total_reads(), MAX_READ_COUNT);
}

// @internal
#[test]
fn reads_older_than_the_window_leave_the_echo() {
    let start = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    trainer.note_peer_frame(&peer_frame(3, &[]), start);
    trainer.note_peer_frame(&peer_frame(8, &[]), start + Duration::from_millis(1500));

    let inside = trainer.next_frame(start + READ_WINDOW - Duration::from_millis(1));
    let after = trainer.next_frame(start + READ_WINDOW + Duration::from_millis(1));
    let long_after = trainer.next_frame(start + READ_WINDOW * 3);

    assert_eq!(inside.echo().count(), 2);
    let echo: Vec<(u8, u8)> = after.echo().map(|r| (r.layout, r.count)).collect();
    assert_eq!(echo, vec![(8, 1)]);
    assert_eq!(long_after.echo().count(), 0);
    assert_eq!(long_after.total_reads(), 0);
}

// @internal
#[test]
fn a_stale_echo_sweeps_again_from_the_last_echoed_layout_not_from_full_size() {
    let start = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    trainer.note_peer_frame(&peer_frame(0, &[(11, 5)]), start);
    assert_eq!(trainer.next_frame(start).layout(), 11);

    let still_fresh = start + ECHO_FRESH - Duration::from_millis(1);
    assert_eq!(trainer.next_frame(still_fresh).layout(), 11);
    assert!(!trainer.is_sweeping(still_fresh));

    let stale = start + ECHO_FRESH + Duration::from_millis(1);
    assert!(trainer.is_sweeping(stale));
    assert_eq!(
        layouts_of_next_frames(&mut trainer, stale, 5),
        vec![11, 12, 13, 0, 1]
    );
    assert_eq!(trainer.last_good_layout(), Some(11));
}

// @internal
#[test]
fn a_new_echo_ends_a_second_sweep() {
    let start = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    trainer.note_peer_frame(&peer_frame(0, &[(11, 5)]), start);
    let stale = start + ECHO_FRESH * 2;
    let _ = layouts_of_next_frames(&mut trainer, stale, 4);

    trainer.note_peer_frame(&peer_frame(0, &[(13, 2)]), stale);

    assert_eq!(layouts_of_next_frames(&mut trainer, stale, 3), vec![13; 3]);
}

// @internal
#[test]
fn a_held_layout_never_changes() {
    let start = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    trainer.note_peer_frame(&peer_frame(0, &[(6, 5)]), start);
    assert_eq!(trainer.next_frame(start).layout(), 6);

    trainer.hold();

    let much_later = start + ECHO_FRESH * 10;
    assert_eq!(
        layouts_of_next_frames(&mut trainer, much_later, 4),
        vec![6; 4]
    );
    assert!(!trainer.is_sweeping(much_later));
    trainer.note_peer_frame(&peer_frame(0, &[(2, 40)]), much_later);
    assert_eq!(trainer.next_frame(much_later).layout(), 6);
}

// @internal
#[test]
fn holding_before_any_echo_keeps_the_layout_last_shown() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(3);
    assert_eq!(layouts_of_next_frames(&mut trainer, now, 2), vec![3, 4]);

    trainer.hold();

    assert_eq!(layouts_of_next_frames(&mut trainer, now, 3), vec![4; 3]);
}

// @internal
#[test]
fn the_sweep_dwell_shortens_only_for_a_peer_that_reads_fast() {
    let now = Instant::now();
    let mut trainer = LinkTrainer::starting_at(0);
    assert_eq!(trainer.sweep_dwell_ms(), SWEEP_DWELL_MS);

    let slow_reader = TrainingHeader::new(0, &[], 12).unwrap();
    trainer.note_peer_frame(&slow_reader, now);
    assert_eq!(trainer.sweep_dwell_ms(), SWEEP_DWELL_MS);

    let fast_reader = TrainingHeader::new(0, &[], FAST_READER_READS).unwrap();
    trainer.note_peer_frame(&fast_reader, now);
    assert_eq!(trainer.sweep_dwell_ms(), FAST_SWEEP_DWELL_MS);
    assert_eq!(FAST_SWEEP_DWELL_MS, 60);
    assert_eq!(SWEEP_DWELL_MS, 100);
}

// @internal
#[test]
fn the_chunk_size_steps_with_the_peers_reported_read_rate() {
    assert_eq!(chunk_bytes_for(0), 34);
    assert_eq!(chunk_bytes_for(STEP_UP_TO_V7_READS - 1), 34);
    assert_eq!(chunk_bytes_for(STEP_UP_TO_V7_READS), 50);
    assert_eq!(chunk_bytes_for(STEP_UP_TO_V8_READS - 1), 50);
    assert_eq!(chunk_bytes_for(STEP_UP_TO_V8_READS), 78);
    assert_eq!(chunk_bytes_for(MAX_READ_COUNT), 78);
    assert_eq!(chunk_bytes_for(u8::MAX), 78);
}
