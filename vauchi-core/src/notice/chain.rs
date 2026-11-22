// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! One notice chain as its writer and its reader hold it.
//!
//! The writer keeps only its next chain key. The reader recognises
//! records up to [`WINDOW`] positions ahead, keeps the keys of positions
//! it skipped for a while, and reads every position at most once.

use std::collections::BTreeMap;

use zeroize::Zeroizing;

use super::keys::{Keyed, chain_step};
use super::seal::{NOTICE_BYTES, PLAINTEXT_BYTES, TAG_BYTES, open};

pub const PERIOD_SECONDS: u64 = 604_800;
/// How far ahead of the lowest position not stepped past a reader
/// recognises a record.
pub const WINDOW: u32 = 512;
/// How many skipped positions of one chain keep their keys.
pub const SKIPPED_KEPT: u32 = 256;
/// How long a skipped position keeps its key.
pub const SKIPPED_SECONDS: u64 = 3 * PERIOD_SECONDS;

pub struct ChainWriter {
    position: u32,
    chain_key: Zeroizing<[u8; 32]>,
}

impl ChainWriter {
    pub fn new(chain_key: &[u8; 32]) -> Self {
        Self {
            position: 0,
            chain_key: Zeroizing::new(*chain_key),
        }
    }

    /// The next position and where a record written there travels. The
    /// chain steps at once, so no position is assigned twice.
    pub fn assign(&mut self) -> (u32, Keyed) {
        let step = chain_step(&self.chain_key);
        let position = self.position;
        self.chain_key = step.next;
        self.position = position.saturating_add(1);
        (position, step.at)
    }
}

struct Skipped {
    at: Keyed,
    skipped_at: u64,
}

pub struct ChainReader {
    /// The lowest position not stepped past.
    position: u32,
    chain_key: Zeroizing<[u8; 32]>,
    /// The tags of the window, from `position` on.
    window_tags: Vec<[u8; TAG_BYTES]>,
    skipped: BTreeMap<u32, Skipped>,
}

impl ChainReader {
    pub fn new(chain_key: &[u8; 32]) -> Self {
        let mut reader = Self {
            position: 0,
            chain_key: Zeroizing::new(*chain_key),
            window_tags: Vec::new(),
            skipped: BTreeMap::new(),
        };
        reader.fill_window();
        reader
    }

    pub fn skipped(&self) -> usize {
        self.skipped.len()
    }

    fn fill_window(&mut self) {
        self.window_tags.clear();
        let mut chain_key = self.chain_key.clone();
        for _ in 0..WINDOW {
            let step = chain_step(&chain_key);
            self.window_tags.push(step.at.tag);
            chain_key = step.next;
        }
    }

    /// Reads a notice if it is a record of this chain not read before:
    /// its position and plaintext. Anything else, a replay and altered
    /// bytes included, returns `None` and changes nothing.
    pub fn read(
        &mut self,
        notice: &[u8; NOTICE_BYTES],
        now: u64,
    ) -> Option<(u32, Zeroizing<[u8; PLAINTEXT_BYTES]>)> {
        let (tag, _) = notice.split_first_chunk::<TAG_BYTES>()?;
        if let Some((position, _)) = self.skipped.iter().find(|(_, s)| &s.at.tag == tag) {
            let position = *position;
            let plaintext = open(&self.skipped.get(&position)?.at, notice)?;
            self.skipped.remove(&position);
            return Some((position, plaintext));
        }
        let ahead = self.window_tags.iter().position(|known| known == tag)?;
        self.read_ahead(ahead, notice, now)
    }

    fn read_ahead(
        &mut self,
        ahead: usize,
        notice: &[u8; NOTICE_BYTES],
        now: u64,
    ) -> Option<(u32, Zeroizing<[u8; PLAINTEXT_BYTES]>)> {
        let mut chain_key = self.chain_key.clone();
        let mut passed = Vec::with_capacity(ahead);
        for _ in 0..ahead {
            let step = chain_step(&chain_key);
            passed.push(step.at);
            chain_key = step.next;
        }
        let step = chain_step(&chain_key);
        // Nothing is kept from the steps above unless the record opens:
        // a seal that fails leaves the chain exactly as it was.
        let plaintext = open(&step.at, notice)?;

        let first_passed = self.position;
        for (offset, at) in passed.into_iter().enumerate() {
            let position = first_passed.saturating_add(u32::try_from(offset).ok()?);
            self.skipped.insert(
                position,
                Skipped {
                    at,
                    skipped_at: now,
                },
            );
        }
        while self.skipped.len() > SKIPPED_KEPT as usize {
            self.skipped.pop_first();
        }
        let read_position = first_passed.saturating_add(u32::try_from(ahead).ok()?);
        self.position = read_position.saturating_add(1);
        self.chain_key = step.next;
        self.fill_window();
        Some((read_position, plaintext))
    }

    /// Erases the keys of positions skipped three periods ago. A stamp
    /// that lies in the future of `now` is set to `now`, so a clock set
    /// back lengthens a key's life and never shortens it.
    pub fn expire(&mut self, now: u64) {
        for skipped in self.skipped.values_mut() {
            if skipped.skipped_at > now {
                skipped.skipped_at = now;
            }
        }
        self.skipped
            .retain(|_, skipped| now.saturating_sub(skipped.skipped_at) < SKIPPED_SECONDS);
    }
}
