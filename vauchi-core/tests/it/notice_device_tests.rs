// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whole devices through Session::write, several directions sharing one
//! budget (gate review 15): a gateway without keys, and the limit of a copy
//! that keeps writing (spec section 7).

use proptest::prelude::*;
use vauchi_core::notice::direction::{Alarm, Receiver, Sender};
use vauchi_core::notice::publish::Publisher;
use vauchi_core::notice::session::{BACKGROUND, FAST, Session};

use crate::common::notice_pair::*;

// Gate review 15 (Claude), stated as a limit by the owner (2026-11-23):
// a copy that forges one higher-numbered refresh a week keeps R's own
// refreshes from opening, so healing does not start while it writes. It
// starts once the copy stops: the forged opening leaves the queue after its
// four attempts, and R's next refresh opens.
// @internal
#[test]
fn a_copy_that_keeps_forging_holds_healing_only_while_it_writes() {
    let mut p = pair();
    let seed = p.receiver.seed_epoch();
    let mut to_receiver = Vec::new();
    for week in 1..=3u64 {
        let now = START + week * WEEK;
        let forged = forged_refresh(&p.copied_seed, now, 3, 100 + week as u32, [9; 32], seed);
        let mut refreshes = p.receiver_session(&to_receiver, now);
        refreshes.push(forged);
        to_receiver = p.sender_session(&refreshes, now + 60);
    }
    p.receiver.read(&to_receiver, START + 3 * WEEK + 120);
    assert_eq!(
        p.receiver.current_epoch(),
        seed,
        "held while the copy writes"
    );

    for day in 0..6 * 7u64 {
        let now = START + 4 * WEEK + day * DAY;
        let refreshes = p.receiver_session(&to_receiver, now);
        to_receiver = p.sender_session(&refreshes, now + 60);
    }
    let refreshes = p.receiver_session(&to_receiver, START + 10 * WEEK);
    p.sender_session(&refreshes, START + 10 * WEEK + 60);
    assert_ne!(p.receiver.current_epoch(), seed, "healing started");
    p.assert_healed();
    p.assert_no_alarm();
}

/// One step of a device's session under a keyless gateway.
#[derive(Debug, Clone)]
struct Step {
    hours: u64,
    /// `None`: the sender's device; `Some(i)`: receiver `i`'s device.
    actor: Option<usize>,
    deliver: Vec<(usize, bool)>,
    acknowledged: bool,
    edit: bool,
    fast: bool,
    clock_ahead_days: u64,
}

const DIRECTIONS: usize = 12;

fn step() -> impl Strategy<Value = Step> {
    (
        0u64..72,
        prop::option::of(0..DIRECTIONS),
        proptest::collection::vec((any::<usize>(), any::<bool>()), 0..24),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        0u64..4,
    )
        .prop_map(
            |(hours, actor, deliver, acknowledged, edit, fast, clock_ahead_days)| Step {
                hours,
                actor,
                deliver,
                acknowledged,
                edit,
                fast,
                clock_ahead_days,
            },
        )
}

fn senders(pairs: &mut [Pair]) -> Vec<&mut Sender> {
    pairs.iter_mut().map(|p| &mut p.sender).collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    // Gate review 15 (Claude, Codex, Kimi): the gateway property ran one
    // direction with unlimited writes, so the shared budget, the alternating
    // control turns and the one opening a direction were never under it.
    // Here one device writes to twelve contacts through Session::write, fast
    // sessions giving five control slots for twelve directions, while the
    // gateway drops, reorders, replays, alters and withholds
    // acknowledgements, with clocks up to three days apart. Then seven
    // faithful weeks: no alarm, every direction healed, every contact holds
    // the latest key.
    // @internal
    #[test]
    fn a_keyless_gateway_cannot_stop_a_device_sharing_its_budget(
        steps in proptest::collection::vec(step(), 0..40),
    ) {
        let mut pairs: Vec<Pair> = (0..DIRECTIONS).map(|_| pair()).collect();
        let mut s_session = Session::default();
        let mut r_sessions: Vec<Session> = (0..DIRECTIONS).map(|_| Session::default()).collect();
        let card = vec![4u8; 120];
        let mut edits = 0u8;
        let mut publishers: Vec<Publisher> = Vec::new();
        let mut from_sender: Vec<Notice> = Vec::new();
        let mut from_receivers: Vec<Notice> = Vec::new();
        let mut now = START;

        for s in steps {
            now += s.hours * 3_600;
            let clock = now + s.clock_ahead_days * DAY;
            let pool = if s.actor.is_some() { &from_sender } else { &from_receivers };
            let mut delivered = Vec::new();
            for (pick, altered) in &s.deliver {
                if pool.is_empty() { break; }
                let mut notice = pool[pick % pool.len()];
                if *altered { notice[pick % notice.len()] ^= 1; }
                delivered.push(notice);
            }
            let tier = if s.fast { FAST } else { BACKGROUND };
            match s.actor {
                Some(i) => {
                    let receiver: &mut Receiver = &mut pairs[i].receiver;
                    receiver.read(&delivered, clock);
                    let written = r_sessions[i].write(&mut [], &mut [receiver], &mut [], tier, clock);
                    if s.acknowledged {
                        Session::acknowledge(&mut [], &mut [&mut pairs[i].receiver], &mut [], clock);
                    }
                    from_receivers.extend(written.notices);
                }
                None => {
                    if s.edit {
                        edits = edits.wrapping_add(1);
                        let key = [edits; 32];
                        for p in &mut pairs {
                            p.sender
                                .queue_group_update(3, &key, &vauchi_core::notice::content::commitment(&key, &card))
                                .expect("within 32 groups");
                        }
                        publishers = vec![Publisher::for_group(3, key, &card)];
                    }
                    for p in &mut pairs {
                        p.sender.read(&delivered, clock);
                    }
                    let written = s_session.write(&mut senders(&mut pairs), &mut [], &mut publishers, tier, clock);
                    if s.acknowledged {
                        Session::acknowledge(&mut senders(&mut pairs), &mut [], &mut publishers, clock);
                    }
                    from_sender.extend(written.notices);
                }
            }
            for p in &pairs {
                prop_assert_eq!(p.sender.alarms(), &[] as &[Alarm]);
                prop_assert_eq!(p.receiver.alarms(), &[] as &[Alarm]);
            }
        }

        edits = edits.wrapping_add(1);
        let key = [edits; 32];
        for p in &mut pairs {
            p.sender
                .queue_group_update(3, &key, &vauchi_core::notice::content::commitment(&key, &card))
                .expect("within 32 groups");
        }
        publishers = vec![Publisher::for_group(3, key, &card)];
        let mut arrived = [false; DIRECTIONS];
        // Agreement alone can hold in the seed epoch (gate review 15,
        // Codex): each receiver must move into an epoch opened after this.
        let numbers_before: Vec<u32> = pairs.iter().map(|p| p.receiver.newest_number()).collect();
        let mut to_sender: Vec<Notice> = Vec::new();
        let mut to_receivers: Vec<Notice> = Vec::new();
        // Seven weeks of background sessions every three days, the gateway
        // delivering everything and acknowledging every write.
        now += 4 * DAY;
        for round in 0..18 {
            for (i, p) in pairs.iter_mut().enumerate() {
                arrived[i] |= p
                    .receiver
                    .read(&to_receivers, now)
                    .iter()
                    .any(|read| *read.update_key == key);
                let written = r_sessions[i].write(&mut [], &mut [&mut p.receiver], &mut [], BACKGROUND, now);
                Session::acknowledge(&mut [], &mut [&mut p.receiver], &mut [], now);
                to_sender.extend(written.notices);
            }
            for p in &mut pairs {
                p.sender.read(&to_sender, now + 3_600);
            }
            to_sender.clear();
            // The last round ends on the sender reading the receivers'
            // newest refreshes, so a healed direction is one S follows.
            if round == 17 {
                break;
            }
            let written = s_session.write(&mut senders(&mut pairs), &mut [], &mut publishers, BACKGROUND, now + 3_600);
            Session::acknowledge(&mut senders(&mut pairs), &mut [], &mut publishers, now + 3_600);
            to_receivers = written.notices;
            now += 3 * DAY;
        }
        for (i, p) in pairs.iter().enumerate() {
            prop_assert_eq!(p.sender.alarms(), &[] as &[Alarm]);
            prop_assert_eq!(p.receiver.alarms(), &[] as &[Alarm]);
            prop_assert_eq!(Some(p.sender.writes_in()), p.receiver.newest_name(), "direction {} healed", i);
            prop_assert!(arrived[i], "direction {} got the latest key", i);
            prop_assert!(
                p.receiver.current_number() > numbers_before[i],
                "direction {} moved into an epoch opened after the disruption", i
            );
        }
    }
}
