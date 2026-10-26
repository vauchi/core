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

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    // @internal
    #[test]
    fn three_groups_of_one_is_all_of_three() {
        let groups = GuardianGroups::new(vec![vec![key(1)], vec![key(2)], vec![key(3)]]).unwrap();

        assert_eq!(groups.config().threshold(), 3);
        assert_eq!(groups.config().count(), 3);
        assert_eq!(groups.groups().len(), 3);
    }

    // @internal
    #[test]
    fn shapes_outside_the_bounds_are_refused() {
        let two_groups = vec![vec![key(1)], vec![key(2)]];
        assert_eq!(
            GuardianGroups::new(two_groups),
            Err(KeyShardError::InvalidParameters)
        );

        let empty_group = vec![vec![key(1)], vec![], vec![key(3)]];
        assert_eq!(
            GuardianGroups::new(empty_group),
            Err(KeyShardError::InvalidParameters)
        );

        let ten_members = vec![
            (1..=4).map(key).collect(),
            (5..=8).map(key).collect(),
            vec![key(9), key(10)],
        ];
        assert_eq!(
            GuardianGroups::new(ten_members),
            Err(KeyShardError::InvalidParameters)
        );

        let ten_groups: Vec<Vec<[u8; 32]>> = (1..=10).map(|b| vec![key(b)]).collect();
        assert_eq!(
            GuardianGroups::new(ten_groups),
            Err(KeyShardError::InvalidParameters)
        );
    }

    // @internal
    #[test]
    fn a_guardian_in_two_groups_is_refused() {
        let twice = vec![vec![key(1)], vec![key(2), key(1)], vec![key(3)]];

        assert_eq!(
            GuardianGroups::new(twice),
            Err(KeyShardError::DuplicateGuardian)
        );
    }
}
