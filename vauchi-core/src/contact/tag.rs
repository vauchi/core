// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Contact tags — owner-private annotation vocabulary.
//!
//! A `Tag` is a reusable, owner-private label for organising and searching
//! contacts ("climbing-gym", "berlin-trip"). One tag name is shared across
//! many contacts; one contact carries many tags.
//!
//! `Tag` is distinct from both sibling concepts:
//! - `Group` (`labels/group.rs`) is a *visibility* construct — it resolves to
//!   the card fields a contact receives. A tag has no `visible_fields` and
//!   never affects what a contact receives.
//! - `LocalGroup` (`local_group.rs`) is organisational with no vocabulary
//!   reuse semantics.
//!
//! Key invariant: like `LocalGroup`, `Tag` has no `visible_fields` field, so
//! it is structurally impossible to leak tag membership over the wire. Tags
//! are never serialised into a `CardSnapshot`. See `ADR-051`.

use std::collections::HashSet;

/// An owner-private annotation tag applied to contacts.
///
/// Tags form a shared vocabulary: the same tag (by `id`) is reused across
/// contacts. Membership is the set of contact IDs carrying the tag.
#[derive(Debug, Clone)]
pub struct Tag {
    /// UUID v4 identifier. Stable across renames of `name`.
    pub id: String,
    /// User-visible tag name (the vocabulary entry).
    pub name: String,
    /// Set of contact IDs carrying this tag.
    pub contact_ids: HashSet<String>,
    /// Unix timestamp (seconds) when the tag was created.
    pub created_at: u64,
}

impl Tag {
    /// Creates a new tag with the given name.
    ///
    /// The ID is a freshly generated UUID v4; `contact_ids` starts empty.
    /// `now` is the Unix-epoch timestamp stamped into `created_at`;
    /// production callers pass `storage.clock().unix_seconds()`.
    pub fn new(name: &str, now: u64) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            contact_ids: HashSet::new(),
            created_at: now,
        }
    }

    /// Returns `true` if this tag is applied to the given contact.
    pub fn contains(&self, contact_id: &str) -> bool {
        self.contact_ids.contains(contact_id)
    }

    /// Applies this tag to a contact. Returns `true` if newly added.
    pub fn add_contact(&mut self, contact_id: &str) -> bool {
        self.contact_ids.insert(contact_id.to_string())
    }

    /// Removes this tag from a contact. Returns `true` if it was present.
    pub fn remove_contact(&mut self, contact_id: &str) -> bool {
        self.contact_ids.remove(contact_id)
    }
}
