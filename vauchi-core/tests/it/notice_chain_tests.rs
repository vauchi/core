// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading and writing a notice chain: the window, skipped positions and
//! what the gateway's loss, delay, replay and tampering do to it
//! (protocol section 6; rules B1 and B2; vauchi/private#1281).

use std::collections::BTreeSet;

use proptest::prelude::*;
use vauchi_core::notice::chain::{
    ChainReader, ChainWriter, PERIOD_SECONDS, SKIPPED_KEPT, SKIPPED_SECONDS, WINDOW,
};
use vauchi_core::notice::seal::{NOTICE_BYTES, PLAINTEXT_BYTES, seal};

const CHAIN_KEY: [u8; 32] = [7; 32];
const NOW: u64 = 1_800_000_000;

fn plaintext(position: u32) -> [u8; PLAINTEXT_BYTES] {
    let mut bytes = [0u8; PLAINTEXT_BYTES];
    bytes[..4].copy_from_slice(&position.to_be_bytes());
    bytes
}

/// The honest sender's notices at positions `0..count`, each saying its
/// own position.
fn written(count: u32) -> Vec<[u8; NOTICE_BYTES]> {
    let mut writer = ChainWriter::new(&CHAIN_KEY);
    (0..count)
        .map(|expected| {
            let (position, at) = writer.assign();
            assert_eq!(position, expected);
            seal(&at, &plaintext(position))
        })
        .collect()
}

fn read(reader: &mut ChainReader, notice: &[u8; NOTICE_BYTES], now: u64) -> Option<u32> {
    reader.read(notice, now).map(|(position, opened)| {
        assert_eq!(*opened, plaintext(position));
        position
    })
}

// @internal
#[test]
fn a_reader_reads_each_position_the_writer_wrote_in_order() {
    let notices = written(5);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    for (position, notice) in notices.iter().enumerate() {
        assert_eq!(read(&mut reader, notice, NOW), Some(position as u32));
    }
    assert_eq!(reader.skipped(), 0);
}

// @internal
#[test]
fn a_missing_position_delays_nothing_and_is_read_when_it_arrives() {
    let notices = written(5);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(read(&mut reader, &notices[3], NOW), Some(3));
    assert_eq!(reader.skipped(), 3);
    assert_eq!(read(&mut reader, &notices[4], NOW), Some(4));
    assert_eq!(read(&mut reader, &notices[1], NOW), Some(1));
    assert_eq!(reader.skipped(), 2);
    assert_eq!(read(&mut reader, &notices[0], NOW), Some(0));
    assert_eq!(read(&mut reader, &notices[2], NOW), Some(2));
    assert_eq!(reader.skipped(), 0);
}

// @internal
#[test]
fn a_position_read_once_is_not_recognised_again() {
    let notices = written(4);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(read(&mut reader, &notices[2], NOW), Some(2));
    assert_eq!(read(&mut reader, &notices[0], NOW), Some(0));
    for replayed in [&notices[2], &notices[0]] {
        assert_eq!(read(&mut reader, replayed, NOW), None);
    }
    assert_eq!(reader.skipped(), 1);
    assert_eq!(read(&mut reader, &notices[1], NOW), Some(1));
    assert_eq!(read(&mut reader, &notices[3], NOW), Some(3));
}

// @internal
#[test]
fn bytes_the_gateway_alters_under_a_real_tag_change_nothing() {
    let notices = written(3);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    for position in [2usize, 0] {
        let mut altered = notices[position];
        altered[40] ^= 1;
        assert_eq!(read(&mut reader, &altered, NOW), None);
        assert_eq!(reader.skipped(), 0);
    }
    assert_eq!(read(&mut reader, &notices[2], NOW), Some(2));
    let mut altered_skipped = notices[1];
    altered_skipped[95] ^= 0x80;
    assert_eq!(read(&mut reader, &altered_skipped, NOW), None);
    assert_eq!(read(&mut reader, &notices[1], NOW), Some(1));
    assert_eq!(read(&mut reader, &notices[0], NOW), Some(0));
}

// @internal
#[test]
fn the_window_is_512_positions_from_the_lowest_not_stepped_past() {
    let notices = written(WINDOW + 2);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(read(&mut reader, &notices[WINDOW as usize], NOW), None);
    assert_eq!(read(&mut reader, &notices[0], NOW), Some(0));
    assert_eq!(
        read(&mut reader, &notices[WINDOW as usize], NOW),
        Some(WINDOW)
    );
    assert_eq!(
        read(&mut reader, &notices[WINDOW as usize + 1], NOW),
        Some(WINDOW + 1)
    );
}

// @internal
#[test]
fn beyond_256_skipped_positions_the_lowest_are_erased() {
    let ahead = SKIPPED_KEPT + 10;
    let notices = written(ahead + 1);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(
        read(&mut reader, &notices[ahead as usize], NOW),
        Some(ahead)
    );
    assert_eq!(reader.skipped(), SKIPPED_KEPT as usize);
    for (erased, notice) in notices.iter().enumerate().take(10) {
        assert_eq!(read(&mut reader, notice, NOW), None, "position {erased}");
    }
    assert_eq!(read(&mut reader, &notices[10], NOW), Some(10));
    assert_eq!(
        read(&mut reader, &notices[ahead as usize - 1], NOW),
        Some(ahead - 1)
    );
}

// @internal
#[test]
fn a_skipped_key_is_erased_three_periods_after_the_skip() {
    assert_eq!(SKIPPED_SECONDS, 3 * PERIOD_SECONDS);
    let notices = written(4);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(read(&mut reader, &notices[1], NOW), Some(1));
    assert_eq!(
        read(&mut reader, &notices[3], NOW + PERIOD_SECONDS),
        Some(3)
    );
    assert_eq!(reader.skipped(), 2);

    reader.expire(NOW + SKIPPED_SECONDS - 1);
    assert_eq!(reader.skipped(), 2);
    reader.expire(NOW + SKIPPED_SECONDS);
    assert_eq!(reader.skipped(), 1);
    assert_eq!(read(&mut reader, &notices[0], NOW + SKIPPED_SECONDS), None);
    assert_eq!(
        read(&mut reader, &notices[2], NOW + SKIPPED_SECONDS),
        Some(2)
    );
}

// @internal
#[test]
fn a_clock_set_back_lengthens_a_skipped_keys_life_and_never_shortens_it() {
    let notices = written(2);
    let mut reader = ChainReader::new(&CHAIN_KEY);
    assert_eq!(read(&mut reader, &notices[1], NOW), Some(1));

    let set_back = NOW - 3 * 86_400;
    reader.expire(set_back);
    assert_eq!(reader.skipped(), 1);
    reader.expire(set_back + SKIPPED_SECONDS - 1);
    assert_eq!(reader.skipped(), 1);
    reader.expire(set_back + SKIPPED_SECONDS);
    assert_eq!(reader.skipped(), 0);
}

// @internal
#[test]
fn a_writer_never_assigns_a_position_twice() {
    let mut writer = ChainWriter::new(&CHAIN_KEY);
    let mut tags = BTreeSet::new();
    for expected in 0..300 {
        let (position, at) = writer.assign();
        assert_eq!(position, expected);
        assert!(tags.insert(at.tag), "tag of position {position} repeats");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    // @internal
    #[test]
    fn whatever_the_gateway_does_each_delivered_position_is_read_exactly_once(
        deliveries in proptest::collection::vec((0usize..40, any::<bool>()), 0..120),
    ) {
        let notices = written(40);
        let mut reader = ChainReader::new(&CHAIN_KEY);
        let mut read_positions = BTreeSet::new();
        let mut delivered_intact = BTreeSet::new();
        for (position, tampered) in deliveries {
            let mut notice = notices[position];
            if tampered {
                notice[16 + position % 80] ^= 1;
            } else {
                delivered_intact.insert(position as u32);
            }
            if let Some(read_position) = read(&mut reader, &notice, NOW) {
                prop_assert!(!tampered);
                prop_assert_eq!(read_position, position as u32);
                prop_assert!(read_positions.insert(read_position), "position read twice");
            }
        }
        prop_assert_eq!(read_positions, delivered_intact);
    }
}
