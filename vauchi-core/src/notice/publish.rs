// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Publishing a group's body in content records, and reading bodies from
//! pending update keys (protocol section 4, specification S5;
//! vauchi/private#1281).
//!
//! The sender's `Publisher` holds one group's current body under its
//! update key. The receiver's `Pending` holds, per direction, the update
//! keys it read but whose bodies it has not installed yet.

use std::collections::{BTreeMap, VecDeque};

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::chain::PERIOD_SECONDS;
use super::content::{
    Assembly, COMMITMENT_BYTES, ID_BYTES, RECORD_BYTES, TRANSMISSIONS, open_record, record_count,
    record_id, seal_record,
};
use super::direction::week;
use crate::crypto::random_bytes;

pub type ContentRecord = [u8; RECORD_BYTES];

const PENDING_KEPT_SECONDS: u64 = 14 * 86_400;
const PENDING_A_DIRECTION: usize = 32;
const INSTALLED_REMEMBERED: usize = 32;

/// A stored time in the future of the clock is pulled back to it when
/// next looked at (protocol section 1).
fn age(now: u64, stamp: &mut u64) -> u64 {
    if *stamp > now {
        *stamp = now;
    }
    now - *stamp
}

struct Publication {
    week: u32,
    next_transmission: Vec<u8>,
    acknowledged: Vec<bool>,
    written_unacknowledged: Vec<usize>,
}

impl Publication {
    fn done(&self) -> bool {
        self.acknowledged
            .iter()
            .zip(&self.next_transmission)
            .all(|(acknowledged, next)| *acknowledged || *next >= TRANSMISSIONS)
    }
}

/// One group's body as its sender publishes it.
pub struct Publisher {
    update_key: Zeroizing<[u8; 32]>,
    body: Zeroizing<Vec<u8>>,
    records: u16,
    last_week: Option<u32>,
    current: Option<Publication>,
    queued: bool,
    completed_at: Option<u64>,
}

impl Publisher {
    /// The first publication of an edit: queued at once.
    ///
    /// # Panics
    /// If `body` is longer than eight records hold.
    pub fn new(update_key: [u8; 32], body: &[u8]) -> Self {
        let records = record_count(body.len()).expect("a body of at most eight records");
        Self {
            update_key: Zeroizing::new(update_key),
            body: Zeroizing::new(body.to_vec()),
            records,
            last_week: None,
            current: None,
            queued: true,
            completed_at: None,
        }
    }

    /// The `w` of the last publication under this update key.
    pub fn last_week(&self) -> Option<u32> {
        self.last_week
    }

    /// Queues the weekly publication once the last was completed a
    /// period ago.
    pub fn queue_due(&mut self, now: u64) {
        if self.queued || self.current.is_some() {
            return;
        }
        if let Some(completed_at) = self.completed_at.as_mut()
            && age(now, completed_at) >= PERIOD_SECONDS
        {
            self.queued = true;
        }
    }

    /// The records of this session, at most `slots`. A queued
    /// publication takes S's week only once that week is later than the
    /// last one's, so a publication brought forward by a clock set back
    /// waits instead of taking a week R does not look in yet (gate
    /// review 4).
    pub fn write(&mut self, now: u64, slots: usize) -> Vec<ContentRecord> {
        if let Some(current) = self.current.as_mut() {
            current.written_unacknowledged.clear();
        }
        let this_week = week(now);
        if self.current.is_none()
            && self.queued
            && self.last_week.is_none_or(|last| this_week > last)
        {
            let records = usize::from(self.records);
            self.current = Some(Publication {
                week: this_week,
                next_transmission: vec![0; records],
                acknowledged: vec![false; records],
                written_unacknowledged: Vec::new(),
            });
            self.last_week = Some(this_week);
            self.queued = false;
        }
        let Some(current) = self.current.as_mut() else {
            return Vec::new();
        };
        // Fewest transmissions first, so that with too few slots every
        // record goes once before any goes again (gate review 5).
        let mut order: Vec<usize> = (0..usize::from(self.records))
            .filter(|index| {
                !current.acknowledged[*index] && current.next_transmission[*index] < TRANSMISSIONS
            })
            .collect();
        order.sort_by_key(|index| (current.next_transmission[*index], *index));
        let mut written = Vec::new();
        for index in order {
            if written.len() >= slots {
                break;
            }
            let transmission = current.next_transmission[index];
            let record = seal_record(
                &self.update_key,
                &self.body,
                current.week,
                u16::try_from(index).unwrap_or(u16::MAX),
                transmission,
                &random_bytes(),
            )
            .expect("a body sized when published");
            current.next_transmission[index] = transmission + 1;
            current.written_unacknowledged.push(index);
            written.push(record);
        }
        self.complete_if_done(now);
        written
    }

    /// The relay took this session's write.
    pub fn acknowledge(&mut self, now: u64) {
        if let Some(current) = self.current.as_mut() {
            for index in current.written_unacknowledged.drain(..) {
                current.acknowledged[index] = true;
            }
        }
        self.complete_if_done(now);
    }

    fn complete_if_done(&mut self, now: u64) {
        if self.current.as_ref().is_some_and(Publication::done) {
            self.current = None;
            self.completed_at = Some(now);
        }
    }
}

struct Entry {
    update_key: Zeroizing<[u8; 32]>,
    commitment: [u8; COMMITMENT_BYTES],
    noted_at: u64,
    assembly: Assembly,
}

/// One direction's update keys read and not yet installed, and the
/// commitments installed most recently.
#[derive(Default)]
pub struct Pending {
    entries: VecDeque<Entry>,
    installed: VecDeque<[u8; COMMITMENT_BYTES]>,
}

impl Pending {
    /// Notes an update key read in a notice. False when its commitment
    /// was installed lately or the key is already pending.
    pub fn note(
        &mut self,
        update_key: &[u8; 32],
        commitment: &[u8; COMMITMENT_BYTES],
        now: u64,
    ) -> bool {
        let installed = self
            .installed
            .iter()
            .any(|held| bool::from(held.ct_eq(commitment)));
        let pending = self
            .entries
            .iter()
            .any(|entry| bool::from(entry.update_key.ct_eq(update_key)));
        if installed || pending {
            return false;
        }
        if self.entries.len() >= PENDING_A_DIRECTION {
            self.entries.pop_front();
        }
        self.entries.push_back(Entry {
            update_key: Zeroizing::new(*update_key),
            commitment: *commitment,
            noted_at: now,
            assembly: Assembly::new(*commitment),
        });
        true
    }

    /// Reads a session's content records and returns each body completed
    /// whose commitment and `verify` (the sender's signature) pass.
    pub fn read(
        &mut self,
        records: &[ContentRecord],
        now: u64,
        verify: impl Fn(&[u8]) -> bool,
    ) -> Vec<Zeroizing<Vec<u8>>> {
        self.entries
            .retain_mut(|entry| age(now, &mut entry.noted_at) < PENDING_KEPT_SECONDS);
        let wanted = self.wanted(now);
        for record in records {
            let mut id = [0u8; ID_BYTES];
            id.copy_from_slice(&record[..ID_BYTES]);
            let Some(&(entry, week, index, transmission)) = wanted.get(&id) else {
                continue;
            };
            let Some(entry) = self.entries.get_mut(entry) else {
                continue;
            };
            let Some(chunk) = open_record(&entry.update_key, week, index, transmission, record)
            else {
                continue;
            };
            // A chunk that disagrees with those held is ignored (section 4).
            if chunk.index == index && entry.assembly.add(chunk).is_err() {
                continue;
            }
        }
        let mut bodies = Vec::new();
        let mut kept = VecDeque::new();
        for entry in self.entries.drain(..) {
            if !entry.assembly.missing().is_empty() {
                kept.push_back(entry);
                continue;
            }
            // A body that fails its commitment or signature is erased with
            // its entry and key (section 4).
            if let Some(body) = entry.assembly.complete(&entry.update_key)
                && verify(&body)
            {
                if self.installed.len() >= INSTALLED_REMEMBERED {
                    self.installed.pop_front();
                }
                self.installed.push_back(entry.commitment);
                bodies.push(body);
            }
        }
        self.entries = kept;
        bodies
    }

    /// Every ID wanted this session: weeks two back to one ahead, every
    /// transmission, every index not held.
    fn wanted(&self, now: u64) -> BTreeMap<[u8; ID_BYTES], (usize, u32, u16, u8)> {
        let this_week = week(now);
        let mut wanted = BTreeMap::new();
        for (position, entry) in self.entries.iter().enumerate() {
            for week in this_week.saturating_sub(2)..=this_week.saturating_add(1) {
                for index in entry.assembly.missing() {
                    for transmission in 0..TRANSMISSIONS {
                        let id = record_id(&entry.update_key, week, index, transmission);
                        wanted.insert(id, (position, week, index, transmission));
                    }
                }
            }
        }
        wanted
    }
}
