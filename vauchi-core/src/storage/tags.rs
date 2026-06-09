// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tag storage forwarders.
//!
//! CRUD now lives in [`TagStore`](super::TagStore); these methods forward to it
//! while call sites migrate (Phase 2 of problem record
//! `2026-06-09-storage-per-domain-store-boundaries`).

use crate::contact::Tag;

use super::{Storage, StorageError};

impl Storage {
    /// Forwards to [`TagStore::create_tag`].
    pub fn create_tag(&self, name: &str) -> Result<Tag, StorageError> {
        self.tags().create_tag(name)
    }

    /// Forwards to [`TagStore::save_tag`].
    pub fn save_tag(&self, tag: &Tag) -> Result<(), StorageError> {
        self.tags().save_tag(tag)
    }

    /// Forwards to [`TagStore::get_tag`].
    pub fn get_tag(&self, id: &str) -> Result<Option<Tag>, StorageError> {
        self.tags().get_tag(id)
    }

    /// Forwards to [`TagStore::list_tags`].
    pub fn list_tags(&self) -> Result<Vec<Tag>, StorageError> {
        self.tags().list_tags()
    }

    /// Forwards to [`TagStore::delete_tag`].
    pub fn delete_tag(&self, id: &str) -> Result<bool, StorageError> {
        self.tags().delete_tag(id)
    }

    /// Forwards to [`TagStore::add_to_tag`].
    pub fn add_to_tag(&self, tag_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.tags().add_to_tag(tag_id, contact_id)
    }

    /// Forwards to [`TagStore::remove_from_tag`].
    pub fn remove_from_tag(&self, tag_id: &str, contact_id: &str) -> Result<(), StorageError> {
        self.tags().remove_from_tag(tag_id, contact_id)
    }
}
