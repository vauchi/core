// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A device's session: a fixed number of notice and content slots,
//! whatever the device has to send, real records first and dummies in the
//! rest, in an order drawn at random (protocol section 8; ADR-081,
//! proposed; vauchi/private#1281).
//!
//! The counts are public and the same for every device, so what a session
//! writes says nothing of how much it had to say (P6).

use super::content::RECORD_BYTES;
use super::direction::{Receiver, Sender, Wants};
use super::publish::{ContentRecord, Publisher};
use super::seal::NOTICE_BYTES;
use crate::crypto::random_bytes;

pub type Notice = [u8; NOTICE_BYTES];

/// The slots of one session of a tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Slots {
    pub notices: usize,
    pub contents: usize,
}

/// A background session (ADR-081, owner 2026-11-21).
pub const BACKGROUND: Slots = Slots {
    notices: 80,
    contents: 5,
};
/// An open-app tick (ADR-081, owner 2026-11-20).
pub const FAST: Slots = Slots {
    notices: 40,
    contents: 2,
};

/// Every eighth notice slot goes to control while control waits.
const CONTROL_EVERY: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Grant {
    pub update: usize,
    pub control: usize,
}

/// Shares `slots` among directions listed in turn order: each slot from
/// the update queue, except that every eighth takes from control while
/// control waits, and control fills what updates leave. Within a queue
/// the directions take turns, the first listed first.
pub fn plan(slots: usize, wants: &[Wants]) -> Vec<Grant> {
    let mut grants = vec![Grant::default(); wants.len()];
    let mut next_update = 0;
    let mut next_control = 0;
    let update_left = |grants: &[Grant], i: usize| grants[i].update < wants[i].update;
    let control_left = |grants: &[Grant], i: usize| grants[i].control < wants[i].control;
    for slot in 1..=slots {
        let any_update = (0..wants.len()).any(|i| update_left(&grants, i));
        let any_control = (0..wants.len()).any(|i| control_left(&grants, i));
        let take_control = any_control && (slot % CONTROL_EVERY == 0 || !any_update);
        if take_control {
            let i = next_turn(wants.len(), next_control, |i| control_left(&grants, i));
            grants[i].control += 1;
            next_control = i + 1;
        } else if any_update {
            let i = next_turn(wants.len(), next_update, |i| update_left(&grants, i));
            grants[i].update += 1;
            next_update = i + 1;
        } else {
            break;
        }
    }
    grants
}

fn next_turn(count: usize, from: usize, wants_more: impl Fn(usize) -> bool) -> usize {
    (0..count)
        .map(|step| (from + step) % count)
        .find(|i| wants_more(*i))
        .unwrap_or(0)
}

/// What one session writes: exactly a tier's counts.
pub struct SessionWrite {
    pub notices: Vec<Notice>,
    pub contents: Vec<ContentRecord>,
}

/// A device's sessions over time: when each direction was last served,
/// so that directions take turns, the one served longest ago first.
#[derive(Default)]
pub struct Session {
    last_served: Vec<u64>,
}

impl Session {
    /// Writes one session. `senders` and `receivers` are the device's
    /// directions, in the same order every session; `publishers` its
    /// groups' bodies, new edits first.
    pub fn write(
        &mut self,
        senders: &mut [&mut Sender],
        receivers: &mut [&mut Receiver],
        publishers: &mut [Publisher],
        slots: Slots,
        now: u64,
    ) -> SessionWrite {
        let directions = senders.len() + receivers.len();
        self.last_served.resize(directions, 0);
        let mut wants: Vec<Wants> = senders.iter().map(|sender| sender.wants(now)).collect();
        wants.extend(receivers.iter().map(|receiver| Wants {
            update: 0,
            control: receiver.wants(now),
        }));
        let mut order: Vec<usize> = (0..directions).collect();
        order.sort_by_key(|i| (self.last_served[*i], *i));
        let ordered: Vec<Wants> = order.iter().map(|i| wants[*i]).collect();
        let mut grants = vec![Grant::default(); directions];
        for (place, grant) in plan(slots.notices, &ordered).into_iter().enumerate() {
            grants[order[place]] = grant;
        }

        let mut notices = Vec::with_capacity(slots.notices);
        let mut wrote = vec![false; directions];
        for (i, sender) in senders.iter_mut().enumerate() {
            let written = sender.write_within(now, grants[i].control, grants[i].update);
            wrote[i] = !written.is_empty();
            notices.extend(written);
        }
        for (j, receiver) in receivers.iter_mut().enumerate() {
            let i = senders.len() + j;
            let written = receiver.write_within(now, grants[i].control);
            wrote[i] = !written.is_empty();
            notices.extend(written);
        }
        // A direction is served when it wrote, not when it was granted a
        // slot it could not use (gate review 6).
        for (i, served) in wrote.iter().enumerate() {
            if *served {
                self.last_served[i] = now;
            }
        }
        notices.truncate(slots.notices);
        notices.resize_with(slots.notices, random_bytes::<NOTICE_BYTES>);
        shuffle(&mut notices);

        let mut contents = Vec::with_capacity(slots.contents);
        for publisher in publishers.iter_mut() {
            publisher.queue_due(now);
        }
        let mut publishing: Vec<&mut Publisher> = publishers.iter_mut().collect();
        publishing.sort_by_key(|publisher| publisher.completed_at());
        for publisher in publishing {
            let room = slots.contents.saturating_sub(contents.len());
            contents.extend(publisher.write(now, room));
        }
        contents.resize_with(slots.contents, random_bytes::<RECORD_BYTES>);
        shuffle(&mut contents);
        SessionWrite { notices, contents }
    }

    /// The relay took the session's write.
    pub fn acknowledge(
        senders: &mut [&mut Sender],
        receivers: &mut [&mut Receiver],
        publishers: &mut [Publisher],
        now: u64,
    ) {
        for sender in senders.iter_mut() {
            sender.acknowledge();
        }
        for receiver in receivers.iter_mut() {
            receiver.acknowledge();
        }
        for publisher in publishers.iter_mut() {
            publisher.acknowledge(now);
        }
    }
}

/// Fisher-Yates with the operating system's randomness.
fn shuffle<T>(items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let draw = u64::from_le_bytes(random_bytes::<8>());
        let j = usize::try_from(draw % (i as u64 + 1)).unwrap_or(0);
        items.swap(i, j);
    }
}
