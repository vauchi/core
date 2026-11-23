// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device's session writes a fixed number of slots, whatever it has to
//! send (protocol section 8; ADR-081, proposed; vauchi/private#1281).
//! What does not fit stays queued and advances no counter.

use crate::common::notice_pair::*;
use vauchi_core::notice::content::RECORD_BYTES;
use vauchi_core::notice::direction::Wants;
use vauchi_core::notice::publish::Publisher;
use vauchi_core::notice::seal::NOTICE_BYTES;
use vauchi_core::notice::session::{BACKGROUND, FAST, Grant, Session, plan};

// @internal
#[test]
fn a_record_not_given_a_slot_stays_queued_and_advances_nothing() {
    let mut p = pair();
    let (key, commitment) = update(1);
    p.sender.queue_update(&key, &commitment);
    assert_eq!(
        p.sender.wants(START),
        Wants {
            update: 1,
            control: 0
        }
    );
    // Were a position assigned to the notice left out, 200 such sessions
    // would pass the cap of 128 positions a period and stop the write below.
    for second in 0..200 {
        assert!(p.sender.write_within(START + second, 0, 0).is_empty());
        p.sender.acknowledge();
    }
    let written = p.sender.write_within(START + 60, 0, 1);
    assert_eq!(written.len(), 1);
    p.sender.acknowledge();
    let read = p.receiver.read(&written, START + DAY);
    assert_eq!(
        read.len(),
        1,
        "read at the first position: none was skipped"
    );
    assert_eq!(*read[0].update_key, key);
    assert_eq!(
        p.sender.wants(START + DAY),
        Wants {
            update: 0,
            control: 0
        }
    );
}

// @internal
#[test]
fn a_refresh_copy_not_given_a_slot_waits_and_spends_no_address() {
    let mut p = pair();
    assert_eq!(
        p.receiver.wants(START),
        1,
        "a refresh is due, sealed in the seed"
    );
    // Were an address spent on the copy left out, five such sessions would
    // use the week's four and stop the write below.
    for second in 0..5 {
        assert!(p.receiver.write_within(START + second, 0).is_empty());
        p.receiver.acknowledge();
    }
    let refresh = p.receiver.write_within(START + 60, 1);
    assert_eq!(refresh.len(), 1);
    p.receiver.acknowledge();
    let answer = p.sender_session(&refresh, START + DAY);
    assert_eq!(answer.len(), 1, "the refresh was read and opened");
    p.assert_no_alarm();
}

// @internal
#[test]
fn updates_come_first_and_every_eighth_slot_goes_to_control() {
    let grants = plan(
        80,
        &[Wants {
            update: 100,
            control: 100,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 70,
            control: 10
        }]
    );
    let grants = plan(
        80,
        &[Wants {
            update: 3,
            control: 100,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 3,
            control: 77
        }],
        "control fills the rest"
    );
    let grants = plan(
        80,
        &[Wants {
            update: 100,
            control: 0,
        }],
    );
    assert_eq!(
        grants,
        vec![Grant {
            update: 80,
            control: 0
        }]
    );
}

// @internal
#[test]
fn directions_take_turns_the_first_listed_first() {
    let wants = [
        Wants {
            update: 100,
            control: 0,
        },
        Wants {
            update: 100,
            control: 0,
        },
    ];
    assert_eq!(
        plan(9, &wants),
        vec![
            Grant {
                update: 5,
                control: 0
            },
            Grant {
                update: 4,
                control: 0
            }
        ]
    );
    let wants = [
        Wants {
            update: 1,
            control: 0,
        },
        Wants {
            update: 100,
            control: 0,
        },
    ];
    assert_eq!(
        plan(9, &wants),
        vec![
            Grant {
                update: 1,
                control: 0
            },
            Grant {
                update: 8,
                control: 0
            }
        ]
    );
}

// @internal
#[test]
fn a_session_writes_the_same_slots_whatever_the_device_has_to_send() {
    for (tier, name) in [(BACKGROUND, "background"), (FAST, "fast")] {
        for updates in [false, true] {
            let mut p = pair();
            let mut publishers = Vec::new();
            if updates {
                let (key, commitment) = update(2);
                p.sender.queue_update(&key, &commitment);
                publishers.push(Publisher::new(key, &[7; 1_500]));
            }
            let mut session = Session::default();
            let written = session.write(
                &mut [&mut p.sender],
                &mut [&mut p.receiver],
                &mut publishers,
                tier,
                START,
            );
            assert_eq!(
                written.notices.len(),
                tier.notices,
                "{name}, updates {updates}"
            );
            assert_eq!(
                written.contents.len(),
                tier.contents,
                "{name}, updates {updates}"
            );
            assert!(
                written
                    .notices
                    .iter()
                    .all(|notice| notice.len() == NOTICE_BYTES)
            );
            assert!(
                written
                    .contents
                    .iter()
                    .all(|record| record.len() == RECORD_BYTES)
            );
        }
    }
}

// @internal
#[test]
fn real_records_take_a_random_place_among_the_dummies() {
    let mut places = std::collections::BTreeSet::new();
    for _ in 0..20 {
        let mut p = pair();
        let mut session = Session::default();
        let written = session.write(&mut [], &mut [&mut p.receiver], &mut [], FAST, START);
        let answered: Vec<usize> = written
            .notices
            .iter()
            .enumerate()
            .filter(|(_, notice)| {
                let mut copy = p.sender.clone();
                copy.read(&[**notice], START + DAY);
                !copy.write(START + DAY).is_empty()
            })
            .map(|(place, _)| place)
            .collect();
        assert_eq!(answered.len(), 1, "one real refresh among the dummies");
        places.insert(answered[0]);
    }
    assert!(
        places.len() > 1,
        "the refresh sat at {places:?} in every session"
    );
}
