// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-data wipe orchestrator.
//!
//! Device-info / registry persistence lives in [`DeviceStore`](super::DeviceStore)
//! and sync state in [`SyncStore`](super::SyncStore). `wipe_device_data` stays a
//! cross-cutting `Storage` orchestrator (it spans both domains), like
//! `delete_contact` — see problem record
//! `2026-06-09-storage-per-domain-store-boundaries`.

use super::{Storage, StorageError};

impl Storage {
    /// Wipes all device-specific data from storage.
    ///
    /// Delegates the `device_info` row to [`DeviceStore::clear_device_info`] and
    /// the sync-owned tables to [`SyncStore::wipe_for_device_reset`]. Used
    /// during identity deletion or device unlinking.
    pub fn wipe_device_data(&self) -> Result<(), StorageError> {
        self.device().clear_device_info()?;
        self.sync().wipe_for_device_reset()
    }
}
