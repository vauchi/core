// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The three safeguards against losing the contact list with the phone
//! (vauchi/private#796 plan step 9, D8): a linked device, guardians, or a
//! recent backup. Without any of them a lost phone takes the contact list
//! with it, since there is no relay backup (D8).

use crate::api::error::VauchiResult;

use super::ProfileEngine;

/// A backup older than this no longer counts as a safeguard (D8c).
pub const RECENT_BACKUP_SECS: u64 = 30 * 24 * 60 * 60;

/// Which safeguards the user has; any one of them is enough (D8a).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LossSafeguards {
    /// Another active device holds the identity. Staleness is not judged
    /// yet: nothing tells Core a sibling took our updates (D8d, #1279).
    pub linked_device: bool,
    /// At least one contact is trusted for recovery.
    pub guardians: bool,
    /// A backup was recorded within [`RECENT_BACKUP_SECS`].
    pub recent_backup: bool,
}

impl LossSafeguards {
    #[must_use]
    pub fn any(self) -> bool {
        self.linked_device || self.guardians || self.recent_backup
    }
}

impl ProfileEngine {
    /// The user's safeguards against losing the contact list.
    ///
    /// # Errors
    /// Storage errors.
    pub fn loss_safeguards(&self) -> VauchiResult<LossSafeguards> {
        let now = self.clock.unix_seconds();
        let linked_device = self
            .storage
            .device()
            .load_device_registry()?
            .is_some_and(|registry| registry.active_devices().len() > 1);
        let guardians = !self.guardian_groups()?.is_empty();
        let recent_backup = self
            .load_backup_reminder_state()?
            .last_backup_timestamp
            .is_some_and(|at| now.saturating_sub(at) < RECENT_BACKUP_SECS);
        Ok(LossSafeguards {
            linked_device,
            guardians,
            recent_backup,
        })
    }
}
