// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Guardian groups: recovery needs one member from each group
//! (vauchi/private#815).
//!
//! A grouped backup is one Shamir split with `threshold = count = groups`;
//! a group's share is sealed to every one of its members. Members of one
//! group therefore recover as one — a group with two members has a spare —
//! and no group can recover without the others. All of three guardians is
//! three groups of one.

use std::collections::HashSet;

use super::{KeyShardConfig, KeyShardError};
use crate::crypto::shamir::MAX_COUNT;

/// Guardian signing keys arranged in groups; see the module doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardianGroups {
    groups: Vec<Vec<[u8; 32]>>,
    config: KeyShardConfig,
}

impl GuardianGroups {
    /// Three to nine groups, each with at least one member, at most nine
    /// members in all (the cap on sealed shares, #816), no key in two groups.
    ///
    /// # Errors
    /// [`KeyShardError::InvalidParameters`] for a shape outside those
    /// bounds, [`KeyShardError::DuplicateGuardian`] for a key in two groups.
    pub fn new(groups: Vec<Vec<[u8; 32]>>) -> Result<Self, KeyShardError> {
        let count = u8::try_from(groups.len()).map_err(|_| KeyShardError::InvalidParameters)?;
        let config = KeyShardConfig::new(count, count)?;
        let members: usize = groups.iter().map(Vec::len).sum();
        if groups.iter().any(Vec::is_empty) || members > usize::from(MAX_COUNT) {
            return Err(KeyShardError::InvalidParameters);
        }
        let mut seen = HashSet::with_capacity(members);
        if !groups.iter().flatten().all(|key| seen.insert(*key)) {
            return Err(KeyShardError::DuplicateGuardian);
        }
        Ok(Self { groups, config })
    }

    /// The groups, in the order their shares are issued.
    pub fn groups(&self) -> &[Vec<[u8; 32]>] {
        &self.groups
    }

    /// One share per group, every group needed.
    pub fn config(&self) -> KeyShardConfig {
        self.config
    }
}
