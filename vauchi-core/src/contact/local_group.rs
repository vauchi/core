// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Local organization groups for imported contacts.
//!
//! `LocalGroup` is a **purely local** organizational construct — it has NO
//! outbound sharing semantics, unlike visibility labels which control what
//! fields exchanged contacts see. Local groups exist only to help the user
//! organize imported contacts on their own device.
//!
//! Key invariant: `LocalGroup` has no `visible_fields` field. This makes it
//! structurally impossible to accidentally share group membership over the
//! wire.

use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// A local organization group for imported contacts.
///
/// Unlike visibility labels, local groups have NO outbound sharing semantics.
/// They are purely for the user's local organization and are never transmitted
/// to contacts or synced in ways that reveal membership to outsiders.
#[derive(Debug, Clone)]
pub struct LocalGroup {
    /// UUID v4 identifier.
    pub id: String,
    /// User-visible group name.
    pub name: String,
    /// Set of contact IDs belonging to this group.
    pub contact_ids: HashSet<String>,
    /// Unix timestamp (seconds) when the group was created.
    pub created_at: u64,
}

impl LocalGroup {
    /// Creates a new local group with the given name.
    ///
    /// The ID is a freshly generated UUID v4. `contact_ids` starts empty.
    pub fn new(name: &str) -> Self {
        let id = uuid::Uuid::new_v4().to_string();
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Self {
            id,
            name: name.to_string(),
            contact_ids: HashSet::new(),
            created_at,
        }
    }

    /// Returns `true` if this group contains the given contact.
    pub fn contains(&self, contact_id: &str) -> bool {
        self.contact_ids.contains(contact_id)
    }

    /// Adds a contact to this group. Returns `true` if it was newly added.
    pub fn add_contact(&mut self, contact_id: &str) -> bool {
        self.contact_ids.insert(contact_id.to_string())
    }

    /// Removes a contact from this group. Returns `true` if it was present.
    pub fn remove_contact(&mut self, contact_id: &str) -> bool {
        self.contact_ids.remove(contact_id)
    }
}
