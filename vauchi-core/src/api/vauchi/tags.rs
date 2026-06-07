// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact tag API — owner-private annotation vocabulary (ADR-051).
//!
//! Thin orchestration over the encrypted `tags` storage (see
//! `storage/tags.rs`). Tags are owner-private, never shared, and apply to
//! both `Exchanged` and `Imported` contacts.
//!
//! Tag names are encrypted at rest, so name-based resolution (the
//! autocomplete-or-create path and suggestions) lists and decrypts the
//! vocabulary and matches in Rust — there is no SQL name lookup.

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;
use crate::contact::Tag;

/// Normalises a tag name for comparison and storage: trims surrounding
/// whitespace. Matching is case-insensitive (see [`Vauchi::find_tag_by_name`]),
/// but the original casing of the first use is preserved as the stored name.
fn normalise_name(name: &str) -> &str {
    name.trim()
}

impl Vauchi {
    /// Lists all tags in the owner's vocabulary, oldest first.
    pub fn list_tags(&self) -> VauchiResult<Vec<Tag>> {
        Ok(self.storage.list_tags()?)
    }

    /// Creates a new tag with the given name.
    ///
    /// Rejects an empty/whitespace-only name. Does **not** dedupe — callers
    /// that want autocomplete-or-create semantics use
    /// [`Vauchi::add_tag_to_contact`].
    pub fn create_tag(&self, name: &str) -> VauchiResult<Tag> {
        let name = normalise_name(name);
        if name.is_empty() {
            return Err(VauchiError::InvalidState("Tag name cannot be empty".into()));
        }
        Ok(self.storage.create_tag(name)?)
    }

    /// Deletes a tag from the vocabulary. Tagged contacts simply lose the tag.
    /// Returns `true` if the tag existed.
    pub fn delete_tag(&self, tag_id: &str) -> VauchiResult<bool> {
        Ok(self.storage.delete_tag(tag_id)?)
    }

    /// Returns the existing tag whose name matches `name` (trimmed,
    /// case-insensitive), or `None`. Names are encrypted, so this scans the
    /// decrypted vocabulary.
    pub fn find_tag_by_name(&self, name: &str) -> VauchiResult<Option<Tag>> {
        let needle = normalise_name(name).to_lowercase();
        if needle.is_empty() {
            return Ok(None);
        }
        Ok(self
            .storage
            .list_tags()?
            .into_iter()
            .find(|t| t.name.to_lowercase() == needle))
    }

    /// Applies a tag to a contact by name, **autocomplete-or-create**: reuses
    /// the existing tag whose name matches (trimmed, case-insensitive), or
    /// creates a new one. Idempotent if the contact already carries the tag.
    /// Returns the tag used.
    ///
    /// Works for both exchanged and imported contacts; errors if the contact
    /// does not exist.
    pub fn add_tag_to_contact(&self, contact_id: &str, name: &str) -> VauchiResult<Tag> {
        let name = normalise_name(name);
        if name.is_empty() {
            return Err(VauchiError::InvalidState("Tag name cannot be empty".into()));
        }
        // Validate the contact exists (avoid orphan membership).
        if self.storage.load_contact(contact_id)?.is_none() {
            return Err(VauchiError::ContactNotFound(contact_id.to_string()));
        }

        let tag = match self.find_tag_by_name(name)? {
            Some(existing) => existing,
            None => self.storage.create_tag(name)?,
        };
        self.storage.add_to_tag(&tag.id, contact_id)?;

        // Return the up-to-date tag (with the new membership reflected).
        self.storage
            .get_tag(&tag.id)?
            .ok_or_else(|| VauchiError::NotFound("Tag vanished after creation".into()))
    }

    /// Removes a tag from a contact. Idempotent. Errors if the tag does not
    /// exist. The tag itself stays in the vocabulary (use [`Vauchi::delete_tag`]
    /// to remove it entirely).
    pub fn remove_tag_from_contact(&self, tag_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.remove_from_tag(tag_id, contact_id)?)
    }

    /// Returns the tags applied to a given contact, oldest first.
    pub fn tags_for_contact(&self, contact_id: &str) -> VauchiResult<Vec<Tag>> {
        Ok(self
            .storage
            .list_tags()?
            .into_iter()
            .filter(|t| t.contains(contact_id))
            .collect())
    }

    /// Returns vocabulary tag names matching `prefix` (trimmed,
    /// case-insensitive), oldest first — the suggestion list for the
    /// autocomplete-or-create field. An empty prefix returns the whole
    /// vocabulary.
    pub fn tag_name_suggestions(&self, prefix: &str) -> VauchiResult<Vec<String>> {
        let needle = normalise_name(prefix).to_lowercase();
        Ok(self
            .storage
            .list_tags()?
            .into_iter()
            .filter(|t| needle.is_empty() || t.name.to_lowercase().starts_with(&needle))
            .map(|t| t.name)
            .collect())
    }
}
