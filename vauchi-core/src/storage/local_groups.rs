// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local-group storage forwarders.
//!
//! CRUD now lives in [`GroupStore`](super::GroupStore); these methods forward
//! to it while call sites migrate (Phase 2 of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`).

use crate::contact::LocalGroup;

use super::{Storage, StorageError};

impl Storage {
    /// Forwards to [`GroupStore::create_local_group`].
    pub fn create_local_group(&self, name: &str) -> Result<LocalGroup, StorageError> {
        self.groups().create_local_group(name)
    }

    /// Forwards to [`GroupStore::get_local_group`].
    pub fn get_local_group(&self, id: &str) -> Result<Option<LocalGroup>, StorageError> {
        self.groups().get_local_group(id)
    }

    /// Forwards to [`GroupStore::list_local_groups`].
    pub fn list_local_groups(&self) -> Result<Vec<LocalGroup>, StorageError> {
        self.groups().list_local_groups()
    }

    /// Forwards to [`GroupStore::delete_local_group`].
    pub fn delete_local_group(&self, id: &str) -> Result<bool, StorageError> {
        self.groups().delete_local_group(id)
    }

    /// Forwards to [`GroupStore::add_to_local_group`].
    pub fn add_to_local_group(&self, group_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.groups().add_to_local_group(group_id, contact_id)
    }

    /// Forwards to [`GroupStore::remove_from_local_group`].
    pub fn remove_from_local_group(
        &self,
        group_id: &str,
        contact_id: &str,
    ) -> Result<(), StorageError> {
        self.groups().remove_from_local_group(group_id, contact_id)
    }
}
