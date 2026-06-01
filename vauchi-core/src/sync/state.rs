// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sync State Machine
//!
//! Manages the synchronization state for each contact and coordinates
//! update delivery with offline queuing and retry logic.

use std::collections::{HashMap, HashSet};

/// Synchronization state for a contact.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum SyncState {
    /// Fully synchronized with no pending updates.
    Synced {
        /// Timestamp of last successful sync.
        last_sync: u64,
    },

    /// Updates are pending for this contact.
    Pending {
        /// Number of updates in the queue.
        queued_count: usize,
        /// Timestamp of last sync attempt (if any).
        last_attempt: Option<u64>,
    },

    /// Currently syncing updates.
    Syncing,

    /// Sync failed, will retry.
    Failed {
        /// Error description.
        error: String,
        /// Timestamp when retry will be attempted.
        retry_at: u64,
    },
}

/// Maximum number of nonces to retain before evicting oldest entries (#41).
const MAX_REPLAY_NONCES: usize = 10_000;

/// Detects replay attacks by tracking per-contact nonces and timestamps.
///
/// Each incoming delta includes a random nonce. The detector rejects:
/// 1. Duplicate nonces (exact replay)
/// 2. Deltas with timestamps older than the last accepted timestamp minus a tolerance window
///
/// The nonce set is capped at `MAX_REPLAY_NONCES` to prevent unbounded memory
/// growth (#41). When the cap is reached, nonces for the oldest contacts
/// (by last timestamp) are evicted.
pub struct ReplayDetector {
    /// Set of (contact_id, nonce) pairs already seen.
    seen_nonces: HashSet<(String, [u8; 32])>,
    /// Last accepted timestamp per contact.
    last_timestamps: HashMap<String, u64>,
    /// Maximum acceptable clock skew in seconds.
    max_clock_skew_secs: u64,
}

impl ReplayDetector {
    /// Creates a new replay detector with the given clock skew tolerance.
    pub fn new(max_clock_skew_secs: u64) -> Self {
        ReplayDetector {
            seen_nonces: HashSet::new(),
            last_timestamps: HashMap::new(),
            max_clock_skew_secs,
        }
    }

    /// Creates a replay detector with the default 60-second clock skew tolerance.
    pub fn default_tolerance() -> Self {
        Self::new(60)
    }

    /// Checks whether a delta should be accepted or rejected as a replay.
    ///
    /// Returns `true` if the delta is fresh (not a replay), `false` if it
    /// should be rejected.
    ///
    /// On acceptance, records the nonce and updates the last timestamp.
    pub fn check_replay(&mut self, contact_id: &str, nonce: &[u8; 32], timestamp: u64) -> bool {
        let key = (contact_id.to_string(), *nonce);

        // Check for duplicate nonce
        if self.seen_nonces.contains(&key) {
            return false;
        }

        // Check for timestamp regression
        if let Some(&last_ts) = self.last_timestamps.get(contact_id)
            && timestamp + self.max_clock_skew_secs < last_ts
        {
            return false;
        }

        // Evict oldest contact nonces if at capacity (#41)
        if self.seen_nonces.len() >= MAX_REPLAY_NONCES {
            self.evict_oldest();
        }

        // Accept: record nonce and update timestamp
        self.seen_nonces.insert(key);
        let entry = self
            .last_timestamps
            .entry(contact_id.to_string())
            .or_insert(0);
        if timestamp > *entry {
            *entry = timestamp;
        }
        true
    }

    /// Prunes old nonces to prevent unbounded memory growth.
    ///
    /// Removes all nonces for entries whose timestamp is older than `cutoff`.
    pub fn prune_before(&mut self, cutoff: u64) {
        let stale_contacts: Vec<String> = self
            .last_timestamps
            .iter()
            .filter(|&(_, &ts)| ts < cutoff)
            .map(|(id, _)| id.clone())
            .collect();

        for contact_id in &stale_contacts {
            self.seen_nonces.retain(|(id, _)| id != contact_id);
            self.last_timestamps.remove(contact_id);
        }
    }

    /// Evicts nonces for the contact with the oldest timestamp (#41).
    ///
    /// Called when `seen_nonces` reaches `MAX_REPLAY_NONCES` to keep
    /// memory bounded.
    fn evict_oldest(&mut self) {
        if let Some(oldest_id) = self
            .last_timestamps
            .iter()
            .min_by_key(|&(_, &ts)| ts)
            .map(|(id, _)| id.clone())
        {
            self.seen_nonces.retain(|(id, _)| *id != oldest_id);
            self.last_timestamps.remove(&oldest_id);
        }
    }

    /// Returns the current number of tracked nonces.
    pub fn nonce_count(&self) -> usize {
        self.seen_nonces.len()
    }
}
