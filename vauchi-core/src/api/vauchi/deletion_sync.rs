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

    /// When the scheduled deletion runs with the wipe choice (Leave
    /// Vauchi, #916), tells every other active device to wipe and sends
    /// that in one sync outside the timing gate, before the identity is
    /// revoked and purged. Returns whether it was sent; unsent, it stays
    /// queued for the next sync. Does nothing without the wipe choice.
    pub fn tell_other_devices_to_wipe(&mut self) -> VauchiResult<bool> {
        let wipe = DeletionManager::new(&self.storage)
            .scheduled_wipes_other_devices()
            .map_err(|e| VauchiError::InvalidState(e.to_string()))?;
        if !wipe {
            return Ok(false);
        }
        self.record_sync_item_or_cover(crate::sync::SyncItem::WipeThisDevice {
            timestamp: self.now_timestamp(),
        });
        Ok(self.sync_once_outside_the_gate())
    }

    /// Runs the emergency wipe when another of the identity's devices told
    /// this one to (#916); `sync` calls it once the received items are
    /// applied. Returns whether this device was wiped.
    pub fn apply_pending_wipe(&mut self) -> VauchiResult<bool> {
        if !self
            .wipe_requested
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(false);
        }
        self.perform_emergency_wipe(true)?;
        self.wiped_by_another_device = true;
        Ok(true)
    }

    /// Whether this device was wiped on another device's instruction, so
    /// the app can return to setup (#916).
    pub fn was_wiped_by_another_device(&self) -> bool {
        self.wiped_by_another_device
    }
}
