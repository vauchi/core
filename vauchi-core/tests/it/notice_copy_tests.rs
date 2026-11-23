// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a copy of the receiver's state can read from the relays, which
//! keep every record 120 days (P2 and P3, vauchi/private#1281).
//!
//! Gate review 2 (Codex, Kimi and Claude) found no test that copies a
//! device. Each test here reads with a copy both inside a window and after
//! it, so that it fails if the copy reads nothing at all, or too much.

use crate::common::notice_pair::*;
use vauchi_core::notice::chain::ChainReader;
use vauchi_core::notice::direction::Receiver;
use vauchi_core::notice::keys::opening_address;
use vauchi_core::notice::record::Record;
use vauchi_core::notice::seal::open;

fn keys_read(reader: &mut Receiver, relays: &[Notice], now: u64) -> Vec<[u8; 32]> {
    reader
        .read(relays, now)
        .iter()
        .map(|read| *read.update_key)
        .collect()
}

/// The records among `written` that are the first opening on refresh 1,
/// as a holder of the seed epoch's keys finds them.
fn first_opening(p: &Pair, written: &[Notice]) -> Vec<Notice> {
    let at = opening_address(p.copied_seed.opening_key(), 1, 0);
    written
        .iter()
        .filter(|notice| open(&at, notice).is_some())
        .copied()
        .collect()
}

// Gate review 3 (Codex): a copy holding a refresh's private key is the
// harder case; it reads the epoch opened on that key, and heals only once
// a refresh made after the copy is opened, named and followed.
// @internal
#[test]
fn a_copy_holding_a_refresh_key_reads_until_a_later_refresh_heals_it() {
    let mut p = pair();
    let mut relays = Vec::new();
    let first_refresh = p.receiver_session(&[], START);
    relays.extend(first_refresh.iter().copied());
    let copy = p.receiver.clone();

    let opening = p.sender_session(&first_refresh, START + DAY);
    relays.extend(opening.iter().copied());
    p.receiver_session(&opening, START + 2 * DAY);
    let first_epoch = p.receiver.current_epoch();

    let naming_first = p.receiver_session(&[], START + WEEK);
    relays.extend(naming_first.iter().copied());
    let (in_first, commitment) = update(1);
    p.sender.queue_update(&in_first, &commitment);
    let written = p.sender_session(&naming_first, START + DAY + WEEK);
    relays.extend(written.iter().copied());
    assert_eq!(
        p.sender.writes_in(),
        first_epoch,
        "followed into the copied key's epoch"
    );
    assert!(keys_read(&mut p.receiver, &written, START + 2 * DAY + WEEK).contains(&in_first));
    assert_ne!(
        p.receiver.current_epoch(),
        first_epoch,
        "moved on a refresh made after the copy"
    );

    let naming_second = p.receiver_session(&[], START + 2 * WEEK);
    relays.extend(naming_second.iter().copied());
    let (in_second, commitment) = update(2);
    p.sender.queue_update(&in_second, &commitment);
    let written = p.sender_session(&naming_second, START + DAY + 2 * WEEK);
    relays.extend(written.iter().copied());
    assert_eq!(p.sender.writes_in(), p.receiver.current_epoch(), "healed");
    assert!(keys_read(&mut p.receiver, &written, START + 2 * DAY + 2 * WEEK).contains(&in_second));

    let mut attacker = copy.clone();
    let mut read = keys_read(&mut attacker, &relays, START + 2 * DAY + 2 * WEEK);
    read.extend(keys_read(
        &mut attacker,
        &relays,
        START + 3 * DAY + 2 * WEEK,
    ));
    assert!(
        read.contains(&in_first),
        "the copy reads the epoch opened on its key"
    );
    assert!(
        !read.contains(&in_second),
        "and nothing once a later refresh healed"
    );
}

// @internal
#[test]
fn a_skipped_position_is_readable_to_a_copy_only_within_three_periods() {
    let mut p = pair();
    let (lost, commitment) = update(1);
    p.sender.queue_update(&lost, &commitment);
    let dropped = p.sender_session(&[], START);
    let (newer, commitment) = update(2);
    p.sender.queue_update(&newer, &commitment);
    let delivered = p.sender_session(&[], START + DAY);
    assert_eq!(
        keys_read(&mut p.receiver, &delivered, START + 2 * DAY),
        vec![newer]
    );

    let inside = p.receiver.clone();
    let mut relays = dropped.clone();
    relays.extend(delivered.iter().copied());
    assert_eq!(
        keys_read(&mut inside.clone(), &relays, START + 2 * DAY + WEEK),
        vec![lost],
        "inside the window the copy reads the skipped position"
    );

    for week in 1..=3 {
        p.receiver_session(&[], START + 2 * DAY + week * WEEK + DAY);
    }
    // Read on a clock set back into the window (gate review 3, Codex): the
    // copy's own expiry then erases nothing, so only what R erased counts.
    let outside = p.receiver.clone();
    assert_eq!(
        keys_read(&mut outside.clone(), &relays, START + 2 * DAY + WEEK),
        Vec::<[u8; 32]>::new(),
        "after three periods and a session it reads nothing"
    );
}

// @internal
#[test]
fn an_epoch_left_is_readable_to_a_copy_only_within_three_periods() {
    let mut p = pair();
    let (lost, commitment) = update(1);
    p.sender.queue_update(&lost, &commitment);
    let mut relays = p.sender_session(&[], START);
    let refresh = p.receiver_session(&[], START + DAY);
    let answer = p.sender_session(&refresh, START + 2 * DAY);
    relays.extend(answer.iter().copied());
    let seed = p.receiver.seed_epoch();
    p.receiver_session(&first_opening(&p, &answer), START + 3 * DAY);
    assert_ne!(p.receiver.current_epoch(), seed, "the receiver moved");

    let inside = p.receiver.clone();
    assert!(
        keys_read(&mut inside.clone(), &relays, START + 3 * DAY + WEEK).contains(&lost),
        "inside the window the copy reads the epoch left"
    );

    for week in 1..=3 {
        p.receiver_session(&[], START + 3 * DAY + week * WEEK + DAY);
    }
    let outside = p.receiver.clone();
    assert_eq!(
        keys_read(&mut outside.clone(), &relays, START + 3 * DAY + WEEK),
        Vec::<[u8; 32]>::new(),
        "after three periods and a session it reads nothing there"
    );
}

/// The update keys a copy of the sender's chains reads among `notices`.
fn sender_copy_reads(readers: &mut [ChainReader], notices: &[Notice], now: u64) -> Vec<[u8; 32]> {
    let mut keys = Vec::new();
    for notice in notices {
        for reader in readers.iter_mut() {
            if let Some((_, plaintext)) = reader.read(notice, now)
                && let Ok(Record::UpdateKey { update_key, .. }) = Record::from_plaintext(&plaintext)
            {
                keys.push(*update_key);
            }
        }
    }
    keys
}

// Gate reviews 3 to 5 (Kimi, Claude): no test copied the sender. A copy
// of S holds its writers' chain keys: it reads what S writes in those
// chains, and nothing once S follows R into an epoch opened after the
// copy with a fresh ephemeral key.
// @internal
#[test]
fn a_copy_of_the_sender_reads_its_chain_until_the_sender_follows_a_new_epoch() {
    let mut p = pair();
    let mut readers = p.sender.copied_chain_readers();
    assert_eq!(readers.len(), 1, "the seed epoch's writer");

    let (before, commitment) = update(1);
    p.sender.queue_update(&before, &commitment);
    let in_seed = p.sender_session(&[], START);
    assert_eq!(
        sender_copy_reads(&mut readers, &in_seed, START),
        vec![before]
    );

    let refresh = p.receiver_session(&in_seed, START + DAY);
    let opening = p.sender_session(&refresh, START + 2 * DAY);
    p.receiver_session(&opening, START + 3 * DAY);
    let naming = p.receiver_session(&[], START + DAY + WEEK);
    let (after, commitment) = update(2);
    p.sender.queue_update(&after, &commitment);
    let written = p.sender_session(&naming, START + 2 * DAY + WEEK);
    assert_ne!(
        p.sender.writes_in(),
        p.receiver.seed_epoch(),
        "followed the new epoch"
    );
    assert!(
        keys_read(&mut p.receiver, &written, START + 3 * DAY + WEEK).contains(&after),
        "R reads it"
    );
    assert_eq!(
        sender_copy_reads(&mut readers, &written, START + 3 * DAY + WEEK),
        Vec::<[u8; 32]>::new(),
        "the copy reads nothing in the epoch opened after it"
    );
}
