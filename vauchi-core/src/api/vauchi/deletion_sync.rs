// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduling and cancelling the identity's deletion, journaled for the
//! identity's other devices so each shows the countdown or its end
//! (vauchi/private#723).

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;
use crate::api::deletion::DeletionManager;
use crate::storage::DeletionState;

impl Vauchi {
    /// Schedules the identity's deletion with its grace period and tells the
    /// other devices; the purge at execution erases the guardian set when
    /// `erase_guardian_set` is set (#711).
    pub fn schedule_identity_deletion(&self, erase_guardian_set: bool) -> VauchiResult<()> {
        let manager = DeletionManager::new(&self.storage);
        manager
            .schedule_deletion_erasing_guardian_set(erase_guardian_set)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        if let DeletionState::Scheduled {
            scheduled_at,
            execute_at,
            erase_guardian_set,
        } = manager
            .deletion_state()
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?
        {
            self.record_sync_item_or_cover(crate::sync::SyncItem::DeletionScheduled {
                scheduled_at,
                execute_at,
                erase_guardian_set,
                timestamp: self.now_timestamp(),
            });
        }
        Ok(())
    }

    /// Cancels the scheduled deletion and tells the other devices.
    pub fn cancel_identity_deletion(&self) -> VauchiResult<()> {
        DeletionManager::new(&self.storage)
            .cancel_deletion()
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        self.record_sync_item_or_cover(crate::sync::SyncItem::DeletionCancelled {
            timestamp: self.now_timestamp(),
        });
        Ok(())
    }
}
