// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduling and cancelling the identity's deletion, journaled for the
//! identity's other devices so each shows the countdown or its end
//! (vauchi/private#723).

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;
use crate::api::deletion::{DeletionChoice, DeletionManager};
use crate::storage::DeletionState;

impl Vauchi {
    /// Schedules the identity's deletion with its grace period and tells the
    /// other devices, with what it carries out when it runs: erasing the
    /// guardian set (#711), wiping the other devices (#916).
    pub fn schedule_identity_deletion(&self, choice: DeletionChoice) -> VauchiResult<()> {
        let manager = DeletionManager::new(&self.storage);
        manager
            .schedule_deletion_with(choice)
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        if let DeletionState::Scheduled {
            scheduled_at,
            execute_at,
            erase_guardian_set,
            wipe_other_devices,
        } = manager
            .deletion_state()
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?
        {
            self.record_sync_item_or_cover(crate::sync::SyncItem::DeletionScheduled {
                scheduled_at,
                execute_at,
                erase_guardian_set,
                wipe_other_devices,
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
