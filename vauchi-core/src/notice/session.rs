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
use super::direction::{GroupId, Receiver, Sender, Wants};
use super::publish::{ContentRecord, Publisher};
use super::seal::NOTICE_BYTES;
use crate::crypto::random_bytes;
use vauchi_protocol::cohort::{CohortClass, CohortTier, CohortWriteRequest};

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
    let order: Vec<usize> = (0..wants.len()).collect();
    plan_ordered(slots, wants, &order, &order)
}

/// As `plan`, with each queue's turn order its own: directions take turns
/// in `update_order` for update slots and in `control_order` for control
/// slots (gate review 11, Codex).
pub fn plan_ordered(
    slots: usize,
    wants: &[Wants],
    update_order: &[usize],
    control_order: &[usize],
) -> Vec<Grant> {
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
            let place = next_turn(control_order.len(), next_control, |place| {
                control_left(&grants, control_order[place])
            });
            grants[control_order[place]].control += 1;
            next_control = place + 1;
        } else if any_update {
            let place = next_turn(update_order.len(), next_update, |place| {
                update_left(&grants, update_order[place])
            });
            grants[update_order[place]].update += 1;
            next_update = place + 1;
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

impl SessionWrite {
    /// The session as the relay's cohort writes: notice slots numbered
    /// from 0, then content slots from 0, every slot of the session, real
    /// or dummy, so the requests say nothing of what the device had to say.
    pub fn cohort_writes(
        &self,
        cohort: u32,
        device: &[u8; 32],
        tier: CohortTier,
    ) -> Vec<CohortWriteRequest> {
        use base64::Engine;
        let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
        let device = encode(device);
        let request = |class, slot: usize, record: &[u8]| CohortWriteRequest {
            cohort,
            device: device.clone(),
            tier,
            class,
            slot: u16::try_from(slot).unwrap_or(u16::MAX),
            record: encode(record),
        };
        self.notices
            .iter()
            .enumerate()
            .map(|(slot, notice)| request(CohortClass::Notice, slot, notice))
            .chain(
                self.contents
                    .iter()
                    .enumerate()
                    .map(|(slot, record)| request(CohortClass::Content, slot, record)),
            )
            .collect()
    }
}

/// A device's sessions over time: when each direction was last served,
/// so that directions take turns, the one served longest ago first. Saved
/// like the directions, so a restart keeps the turn order (gate review 9,
/// Codex).
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Session {
    /// When each direction's update queue and control queue was last
    /// served: an update is no turn of the control queue (gate review 11).
    served_update: Vec<u64>,
    served_control: Vec<u64>,
}

impl Session {
    /// Shares `slots` among directions, each queue taking turns by when
    /// it was last served, the one served longest ago first.
    pub fn plan_turns(&mut self, slots: usize, wants: &[Wants]) -> Vec<Grant> {
        let directions = wants.len();
        self.served_update.resize(directions, 0);
        self.served_control.resize(directions, 0);
        let mut update_order: Vec<usize> = (0..directions).collect();
        update_order.sort_by_key(|i| (self.served_update[*i], *i));
        let mut control_order: Vec<usize> = (0..directions).collect();
        control_order.sort_by_key(|i| (self.served_control[*i], *i));
        plan_ordered(slots, wants, &update_order, &control_order)
    }

    /// Records which queues of which directions wrote at `now`.
    pub fn record_turns(&mut self, wrote: &[Grant], now: u64) {
        self.served_update.resize(wrote.len(), 0);
        self.served_control.resize(wrote.len(), 0);
        for (i, grant) in wrote.iter().enumerate() {
            if grant.update > 0 {
                self.served_update[i] = now;
            }
            if grant.control > 0 {
                self.served_control[i] = now;
            }
        }
    }

    pub fn save(&self) -> Result<zeroize::Zeroizing<Vec<u8>>, super::saved::SavedError> {
        super::saved::save(super::saved::Kind::Session, self)
    }

    pub fn load(bytes: &[u8]) -> Result<Self, super::saved::SavedError> {
        super::saved::load(super::saved::Kind::Session, bytes)
    }

    /// Writes one session. `senders` and `receivers` are the device's
    /// directions, in the same order every session; `publishers` its
    /// groups' bodies, served by how long each has waited.
    pub fn write(
        &mut self,
        senders: &mut [&mut Sender],
        receivers: &mut [&mut Receiver],
        publishers: &mut [Publisher],
        slots: Slots,
        now: u64,
    ) -> SessionWrite {
        // Content first: a new edit's notice goes only once record 0 of its
        // publication is written, in this session or before (section 4).
        let contents = write_contents(publishers, slots.contents, now);
        let eligible = |group: GroupId| {
            publishers
                .iter()
                .any(|publisher| publisher.group() == group && publisher.first_record_written())
        };

        let directions = senders.len() + receivers.len();
        let mut wants: Vec<Wants> = senders
            .iter()
            .map(|sender| sender.wants_for(now, eligible))
            .collect();
        wants.extend(receivers.iter().map(|receiver| Wants {
            update: 0,
            control: receiver.wants(now),
        }));
        let grants = self.plan_turns(slots.notices, &wants);

        let mut notices = Vec::with_capacity(slots.notices);
        let mut wrote = vec![Grant::default(); directions];
        for (i, sender) in senders.iter_mut().enumerate() {
            let written =
                sender.write_within_for(now, grants[i].control, grants[i].update, eligible);
            let counts = sender.last_written();
            wrote[i] = Grant {
                update: counts.update,
                control: counts.control,
            };
            notices.extend(written);
        }
        for (j, receiver) in receivers.iter_mut().enumerate() {
            let i = senders.len() + j;
            let written = receiver.write_within(now, grants[i].control);
            wrote[i] = Grant {
                update: 0,
                control: written.len(),
            };
            notices.extend(written);
        }
        // A queue is served when it wrote, not when it was granted a slot
        // it could not use (gate review 6).
        self.record_turns(&wrote, now);
        notices.truncate(slots.notices);
        notices.resize_with(slots.notices, random_bytes::<NOTICE_BYTES>);
        shuffle(&mut notices);

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

/// The session's content slots: the publication waiting longest first,
/// new edits first among those waiting as long (owner, 2026-11-23, gate
/// review 10); dummies in the rest, in random order.
fn write_contents(publishers: &mut [Publisher], slots: usize, now: u64) -> Vec<ContentRecord> {
    let mut contents = Vec::with_capacity(slots);
    for publisher in publishers.iter_mut() {
        publisher.queue_due(now);
    }
    let mut publishing: Vec<&mut Publisher> = publishers.iter_mut().collect();
    publishing.sort_by_key(|publisher| {
        (
            publisher.waiting_since().unwrap_or(u64::MAX),
            publisher.completed_at().is_some(),
        )
    });
    for publisher in publishing {
        let room = slots.saturating_sub(contents.len());
        contents.extend(publisher.write(now, room));
    }
    contents.resize_with(slots, random_bytes::<RECORD_BYTES>);
    shuffle(&mut contents);
    contents
}

/// Fisher-Yates with the operating system's randomness.
fn shuffle<T>(items: &mut [T]) {
    for i in (1..items.len()).rev() {
        let draw = u64::from_le_bytes(random_bytes::<8>());
        let j = usize::try_from(draw % (i as u64 + 1)).unwrap_or(0);
        items.swap(i, j);
    }
}
