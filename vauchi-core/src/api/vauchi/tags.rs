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
        Ok(self.storage.tags().list_tags()?)
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
        Ok(self.storage.tags().create_tag(name)?)
    }

    /// Deletes a tag from the vocabulary. Tagged contacts simply lose the tag.
    /// Returns `true` if the tag existed.
    pub fn delete_tag(&self, tag_id: &str) -> VauchiResult<bool> {
        Ok(self.storage.tags().delete_tag(tag_id)?)
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
            .tags()
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
        if self.storage.contacts().load_contact(contact_id)?.is_none() {
            return Err(VauchiError::ContactNotFound(contact_id.to_string()));
        }

        let tag = match self.find_tag_by_name(name)? {
            Some(existing) => existing,
            None => self.storage.tags().create_tag(name)?,
        };
        self.storage.tags().add_to_tag(&tag.id, contact_id)?;

        // Return the up-to-date tag (with the new membership reflected).
        self.storage
            .tags()
            .get_tag(&tag.id)?
            .ok_or_else(|| VauchiError::NotFound("Tag vanished after creation".into()))
    }

    /// Removes a tag from a contact. Idempotent. Errors if the tag does not
    /// exist. The tag itself stays in the vocabulary (use [`Vauchi::delete_tag`]
    /// to remove it entirely).
    pub fn remove_tag_from_contact(&self, tag_id: &str, contact_id: &str) -> VauchiResult<()> {
        Ok(self.storage.tags().remove_from_tag(tag_id, contact_id)?)
    }

    /// Returns the tags applied to a given contact, oldest first.
    pub fn tags_for_contact(&self, contact_id: &str) -> VauchiResult<Vec<Tag>> {
        Ok(self
            .storage
            .tags()
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
            .tags()
            .list_tags()?
            .into_iter()
            .filter(|t| needle.is_empty() || t.name.to_lowercase().starts_with(&needle))
            .map(|t| t.name)
            .collect())
    }
}

/// An unsaved preview of the `Group` that promoting a tag would create
/// (T1.4, ADR-051). Returned by [`Vauchi::begin_tag_promotion`] — **nothing is
/// persisted** until [`Vauchi::confirm_tag_promotion`]. The owner reviews and
/// may edit `visible_fields` on a confirmation screen before confirming;
/// discarding the draft (not confirming) is the cancel path.
#[derive(Debug, Clone)]
pub struct GroupDraft {
    /// The tag being promoted (needed to confirm/consume it).
    pub tag_id: String,
    /// Proposed group name (the tag's name).
    pub name: String,
    /// Contacts carrying the tag — the prospective group members (sorted).
    pub contact_ids: Vec<String>,
    /// Field visibility inherited from the owner's current defaults (the own
    /// card's "everyone" fields), sorted. Editable before confirm.
    pub visible_fields: Vec<String>,
}

impl Vauchi {
    /// Begins promoting a tag to a visibility `Group`: returns an **unsaved**
    /// [`GroupDraft`] (members = tagged contacts; `visible_fields` inherited
    /// from the owner's current default-visible card fields). No `Group` is
    /// created and the tag is untouched until [`Vauchi::confirm_tag_promotion`].
    /// Errors if the tag does not exist.
    pub fn begin_tag_promotion(&self, tag_id: &str) -> VauchiResult<GroupDraft> {
        let tag = self
            .storage
            .tags()
            .get_tag(tag_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("tag: {tag_id}")))?;

        let mut contact_ids: Vec<String> = tag.contact_ids.into_iter().collect();
        contact_ids.sort();

        let mut visible_fields: Vec<String> = match self.storage.contacts().load_own_card()? {
            Some(card) => card
                .field_visibility()
                .everyone_field_ids()
                .into_iter()
                .collect(),
            None => Vec::new(),
        };
        visible_fields.sort();

        Ok(GroupDraft {
            tag_id: tag_id.to_string(),
            name: tag.name,
            contact_ids,
            visible_fields,
        })
    }

    /// Confirms a tag-to-group promotion (**replace** semantics): creates a
    /// `Group` named after the tag, adds the tagged contacts as members, sets
    /// the reviewed `visible_fields`, then **deletes the tag**. Returns the new
    /// group id. Errors if the tag no longer exists.
    ///
    /// `visible_fields` is the owner's reviewed set from the confirmation
    /// screen (which may differ from the inherited defaults in the draft).
    /// The group is fully created before the tag is removed; if the (simple)
    /// tag delete fails, the group still exists and the tag can be removed
    /// manually.
    pub fn confirm_tag_promotion(
        &self,
        tag_id: &str,
        visible_fields: Vec<String>,
    ) -> VauchiResult<String> {
        let tag = self
            .storage
            .tags()
            .get_tag(tag_id)?
            .ok_or_else(|| VauchiError::NotFound(format!("tag: {tag_id}")))?;

        let now = self.clock.unix_seconds();
        let mut group = self.storage.labels().create_group(&tag.name)?;
        for contact_id in &tag.contact_ids {
            group.add_contact(contact_id, now);
        }
        group.set_visible_fields(visible_fields.into_iter().collect(), now);
        self.storage.labels().save_group(&group)?;

        // Replace: consume the tag now that the group is fully persisted.
        self.storage.tags().delete_tag(tag_id)?;

        Ok(group.id().to_string())
    }
}
