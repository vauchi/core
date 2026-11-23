// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One direction of the notice core as its two devices hold it: the
//! receiver, which refreshes and moves, and the sender, which follows,
//! opens and writes.
//!
//! Without content and without the slot budget: every record a device
//! wants to write in a session is written. A device's sessions are
//! `read`, then `write`, then `acknowledge` when the relay took the write.

use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::chain::PERIOD_SECONDS;
use super::seal::{NOTICE_BYTES, TAG_BYTES};

/// How many addresses a refresh has in one epoch and week, and an
/// opening under one number in one base.
pub const ATTEMPTS: u8 = 4;
pub(super) const RETENTION_SECONDS: u64 = 3 * PERIOD_SECONDS;
pub(super) const REFRESH_EVERY_SECONDS: u64 = PERIOD_SECONDS;
/// The relays keep records 120 days; eighteen weeks cover that.
pub(super) const WEEKS_WATCHED_BACK: u32 = 18;
/// As long as a refresh's addresses are watched, so that a replay inside
/// the watch is always known.
pub(super) const REFRESH_MEMORY_SECONDS: u64 = (WEEKS_WATCHED_BACK as u64 + 2) * PERIOD_SECONDS;
pub(super) const NUMBERS_WATCHED_AHEAD: u32 = 2;
pub(super) const REFRESH_KEYS_HELD: usize = 3;
pub(super) const REFRESH_KEYS_HELD_BY_RECEIVER: u32 = REFRESH_KEYS_HELD as u32;
pub(super) const NAMED_EPOCHS_KEPT: usize = 24;
pub(super) const UNNAMED_EPOCHS_KEPT: usize = 12;
pub(super) const ERASED_IDS_KEPT: usize = 64;
pub(super) const REFRESHES_REMEMBERED: usize = 64;
pub(super) const REFRESHES_A_SESSION: usize = 8;
/// A quarter of the reader's window, so withheld acknowledgements take
/// four periods to run a chain past it.
pub(super) const POSITIONS_A_PERIOD: usize = 128;

pub type EpochId = [u8; 16];
/// A contact group as Core names it; opaque to the notice core.
pub type GroupId = u32;
/// A contact is in at most 32 of a user's groups, so a receiver's 32
/// pending keys and 32 installed commitments hold every group's latest
/// (owner, 2026-11-23, gate review 9).
pub const GROUPS_A_CONTACT: usize = 32;

/// A 33rd group for one contact is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooManyGroups;
pub(super) type Notice = [u8; NOTICE_BYTES];

pub fn week(now: u64) -> u32 {
    u32::try_from(now / PERIOD_SECONDS).unwrap_or(u32::MAX)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Alarm {
    OpeningOnNumberNotIssued,
    OpeningKeyIdNotIssued,
    OpeningConfirmationFailed,
    TwoOpeningsOnOneNumber,
    RefreshConflict,
    ForeignName,
    TwoRecordsAtOneAddress,
}

/// What one direction would write this session, by queue (protocol
/// section 8): update-key notices of new edits, and control records
/// (refresh copies, openings, a view).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Wants {
    pub update: usize,
    pub control: usize,
}

/// An update key the receiver read, to be matched with its content.
pub struct UpdateKeyRead {
    pub update_key: Zeroizing<[u8; 32]>,
    pub commitment: [u8; 31],
}

pub(super) enum Seen {
    First,
    Again,
    Different,
}

/// What a device first read at each address: a replay is dropped, and a
/// second, different record there can only come from a holder of the
/// epoch's keys. An entry is kept while its address is watched.
#[derive(Default)]
#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FirstRecords(BTreeMap<[u8; TAG_BYTES], FirstRecord>);

#[cfg_attr(feature = "testing", derive(Clone))]
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct FirstRecord {
    hash: [u8; 32],
    read_at: u64,
    /// The week in a refresh address, which says when it stops being
    /// watched; `None` for an opening address, watched by age.
    address_week: Option<u32>,
}

/// The whole SHA-256 of a record: a key holder choosing both records must
/// not find two that compare equal (gate review 2, Codex).
pub(super) fn record_hash(notice: &Notice) -> [u8; 32] {
    Sha256::digest(notice).into()
}

impl FirstRecords {
    pub(super) fn seen(&self, tag: &[u8; TAG_BYTES], notice: &Notice) -> Seen {
        match self.0.get(tag) {
            None => Seen::First,
            Some(first) if first.hash == record_hash(notice) => Seen::Again,
            Some(_) => Seen::Different,
        }
    }

    pub(super) fn remember(
        &mut self,
        tag: [u8; TAG_BYTES],
        notice: &Notice,
        now: u64,
        address_week: Option<u32>,
    ) {
        self.0.entry(tag).or_insert(FirstRecord {
            hash: record_hash(notice),
            read_at: now,
            address_week,
        });
    }

    pub(super) fn forget_unwatched(&mut self, now: u64) {
        let this_week = week(now);
        self.0.retain(|_, first| match first.address_week {
            Some(address_week) => this_week <= address_week.saturating_add(WEEKS_WATCHED_BACK),
            // An opening's number is watched for three periods from its
            // refresh, which was made before the opening was read.
            None => age(now, &mut first.read_at) < RETENTION_SECONDS,
        });
    }
}

pub(super) fn age(now: u64, stamp: &mut u64) -> u64 {
    if *stamp > now {
        *stamp = now;
    }
    now.saturating_sub(*stamp)
}

/// A device's state is copied whole only as an attacker who copies its
/// storage holds it: the tests of P2 and P3 read with such a copy.
#[cfg(feature = "testing")]
pub(super) fn copy_key(key: &crate::crypto::X25519KeyPair) -> crate::crypto::X25519KeyPair {
    crate::crypto::X25519KeyPair::from_bytes(*key.secret_bytes())
}

pub(super) fn tag_of(notice: &Notice) -> [u8; TAG_BYTES] {
    let mut tag = [0u8; TAG_BYTES];
    for (to, from) in tag.iter_mut().zip(notice.iter()) {
        *to = *from;
    }
    tag
}

mod receiver;
mod sender;

pub use receiver::Receiver;
pub use sender::Sender;
