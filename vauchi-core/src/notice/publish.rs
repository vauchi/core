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
use super::direction::{GroupId, week};
use crate::crypto::random_bytes;

pub type ContentRecord = [u8; RECORD_BYTES];

/// A week longer than the longest absence the specification allows (14
/// days), so a renewal is never lost on the boundary (owner, 2026-11-23).
const PENDING_KEPT_SECONDS: u64 = 21 * 86_400;
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

#[derive(serde::Serialize, serde::Deserialize)]
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
#[derive(serde::Serialize, serde::Deserialize)]
pub struct Publisher {
    group: GroupId,
    update_key: Zeroizing<[u8; 32]>,
    body: Zeroizing<Vec<u8>>,
    records: u16,
    last_week: Option<u32>,
    current: Option<Publication>,
    queued: bool,
    completed_at: Option<u64>,
    /// Record 0 of this key's first publication was written: its notice
    /// may go (section 4).
    first_record_written: bool,
    /// Since when the queued or open publication has waited: content
    /// slots go to the one waiting longest (owner, 2026-11-23, gate review
    /// 10).
    waiting_since: Option<u64>,
}

impl Publisher {
    /// The first publication of an edit: queued at once.
    ///
    /// # Panics
    /// If `body` is longer than eight records hold.
    pub fn new(update_key: [u8; 32], body: &[u8]) -> Self {
        Self::for_group(0, update_key, body)
    }

    /// The first publication of an edit of `group`: queued at once.
    ///
    /// # Panics
    /// If `body` is longer than eight records hold.
    pub fn for_group(group: GroupId, update_key: [u8; 32], body: &[u8]) -> Self {
        let records = record_count(body.len()).expect("a body of at most eight records");
        Self {
            group,
            first_record_written: false,
            waiting_since: None,
            update_key: Zeroizing::new(update_key),
            body: Zeroizing::new(body.to_vec()),
            records,
            last_week: None,
            current: None,
            queued: true,
            completed_at: None,
        }
    }

    pub fn save(&self) -> Result<Zeroizing<Vec<u8>>, super::saved::SavedError> {
        super::saved::save(super::saved::Kind::Publisher, self)
    }

    pub fn load(bytes: &[u8]) -> Result<Self, super::saved::SavedError> {
        super::saved::load(super::saved::Kind::Publisher, bytes)
    }

    pub fn group(&self) -> GroupId {
        self.group
    }

    /// Whether record 0 of this key has been written, so that the edit's
    /// notice may go.
    /// Whether this publisher holds `update_key`'s body.
    pub fn publishes(&self, update_key: &[u8; 32]) -> bool {
        bool::from(self.update_key.ct_eq(update_key))
    }

    pub fn first_record_written(&self) -> bool {
        self.first_record_written
    }

    /// The `w` of the last publication under this update key.
    pub fn last_week(&self) -> Option<u32> {
        self.last_week
    }

    /// When the last publication was completed; `None` for an edit not
    /// yet published once, which goes first (section 4).
    pub fn completed_at(&self) -> Option<u64> {
        self.completed_at
    }

    /// Queues the weekly publication once the last was completed a
    /// period ago.
    pub fn queue_due(&mut self, now: u64) {
        // A stored time in the future is pulled back to the clock (section
        // 1; gate review 11, Claude).
        if let Some(waiting_since) = self.waiting_since.as_mut() {
            age(now, waiting_since);
        }
        if self.queued || self.current.is_some() {
            if self.waiting_since.is_none() {
                self.waiting_since = Some(now);
            }
            return;
        }
        if let Some(completed_at) = self.completed_at.as_mut()
            && age(now, completed_at) >= PERIOD_SECONDS
        {
            self.queued = true;
            self.waiting_since = Some(completed_at.saturating_add(PERIOD_SECONDS).min(now));
        }
    }

    /// Since when this publisher's next record has waited; `None` when it
    /// has nothing due.
    pub fn waiting_since(&self) -> Option<u64> {
        self.waiting_since
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
        // The week is fixed when the first record is assigned (section 4):
        // a publication given no slot takes none.
        if self.current.is_none()
            && slots > 0
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
        // R looks back two weeks only: a publication more than a week
        // behind S's week writes what it still has under S's week, a week
        // never used for this key (gate review 7, Kimi). The chunks are the
        // same in every publication, so R completes the body from both.
        if slots > 0 && this_week > current.week.saturating_add(1) {
            current.week = this_week;
            self.last_week = Some(this_week);
        }
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
            if index == 0 {
                self.first_record_written = true;
            }
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
            self.waiting_since = None;
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    update_key: Zeroizing<[u8; 32]>,
    commitment: [u8; COMMITMENT_BYTES],
    noted_at: u64,
    assembly: Assembly,
}

/// One direction's update keys read and not yet installed, and the
/// commitments installed most recently.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Pending {
    entries: VecDeque<Entry>,
    installed: VecDeque<[u8; COMMITMENT_BYTES]>,
}

impl Pending {
    pub fn save(&self) -> Result<Zeroizing<Vec<u8>>, super::saved::SavedError> {
        super::saved::save(super::saved::Kind::Pending, self)
    }

    /// Loads saved state and erases entries 21 days old first.
    pub fn load(bytes: &[u8], now: u64) -> Result<Self, super::saved::SavedError> {
        let mut pending: Self = super::saved::load(super::saved::Kind::Pending, bytes)?;
        pending.expire(now);
        Ok(pending)
    }

    /// Erases entries whose time is 21 days old, with their keys and chunks.
    fn expire(&mut self, now: u64) {
        self.entries
            .retain_mut(|entry| age(now, &mut entry.noted_at) < PENDING_KEPT_SECONDS);
    }

    /// Notes an update key read in a notice. False when its commitment
    /// was installed lately or the key is already pending with it; a key already
    /// pending is kept 21 days from this notice, with its chunks, so that a
    /// card whose publication outlasts 21 days still completes (owner,
    /// 2026-11-23, gate review 6). A superseded key is not sent again, so
    /// it goes 21 days after its last notice.
    pub fn note(
        &mut self,
        update_key: &[u8; 32],
        commitment: &[u8; COMMITMENT_BYTES],
        now: u64,
    ) -> bool {
        self.expire(now);
        let installed = self
            .installed
            .iter()
            .any(|held| bool::from(held.ct_eq(commitment)));
        if installed {
            return false;
        }
        // Found by key and commitment: an honest sender never sends one key
        // with two commitments, so a second one is a copy's, and must not
        // keep the honest view from being noted (gate 16, Claude).
        if let Some(entry) = self.entries.iter_mut().find(|entry| {
            bool::from(entry.update_key.ct_eq(update_key) & entry.commitment.ct_eq(commitment))
        }) {
            entry.noted_at = now.max(entry.noted_at);
            return false;
        }
        if self.entries.len() >= PENDING_A_DIRECTION
            && let Some(oldest) = (0..self.entries.len()).min_by_key(|i| self.entries[*i].noted_at)
        {
            // The oldest by its time, which a view renews (section 4).
            self.entries.remove(oldest);
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
        self.expire(now);
        let wanted = self.wanted(now);
        // Record 0 first: it carries the hashes the later chunks are
        // checked against, whatever order they came in.
        let mut ordered: Vec<(&ContentRecord, u16)> = records
            .iter()
            .filter_map(|record| {
                let mut id = [0u8; ID_BYTES];
                id.copy_from_slice(&record[..ID_BYTES]);
                wanted.get(&id).map(|found| (record, found.2))
            })
            .collect();
        ordered.sort_by_key(|(_, index)| *index);
        for (record, _) in ordered {
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
            // A chunk that does not check out is ignored and changes nothing
            // (section 4): a forged record cannot lock the body out.
            if chunk.index == index && entry.assembly.add(&entry.update_key, chunk).is_err() {
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
            // A body whose signature fails is erased with its entry and key
            // (section 4); every chunk was checked as it came.
            if let Some(body) = entry.assembly.complete()
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
