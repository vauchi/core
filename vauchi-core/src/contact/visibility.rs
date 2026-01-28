// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Rules for Contact Fields
//!
//! Controls which contacts can see which fields on your contact card.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Visibility setting for a single field.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldVisibility {
    /// Visible to everyone (default for new fields)
    #[default]
    Everyone,
    /// Visible only to specific contacts
    Contacts(HashSet<String>),
    /// Visible to no one (private)
    Nobody,
}

/// Visibility rules for all fields in a contact card.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct VisibilityRules {
    /// Map from field ID to visibility setting
    rules: HashMap<String, FieldVisibility>,
}

impl VisibilityRules {
    /// Creates a new empty visibility rules set.
    pub fn new() -> Self {
        VisibilityRules {
            rules: HashMap::new(),
        }
    }

    /// Gets the visibility for a field.
    ///
    /// Returns `Everyone` if no specific rule is set.
    pub fn get(&self, field_id: &str) -> &FieldVisibility {
        self.rules
            .get(field_id)
            .unwrap_or(&FieldVisibility::Everyone)
    }

    /// Sets visibility for a field to everyone.
    pub fn set_everyone(&mut self, field_id: &str) {
        self.rules
            .insert(field_id.to_string(), FieldVisibility::Everyone);
    }

    /// Sets visibility for a field to specific contacts only.
    pub fn set_contacts(&mut self, field_id: &str, contact_ids: HashSet<String>) {
        self.rules
            .insert(field_id.to_string(), FieldVisibility::Contacts(contact_ids));
    }

    /// Sets visibility for a field to nobody (private).
    pub fn set_nobody(&mut self, field_id: &str) {
        self.rules
            .insert(field_id.to_string(), FieldVisibility::Nobody);
    }

    /// Removes the visibility rule for a field (reverts to default).
    pub fn remove(&mut self, field_id: &str) {
        self.rules.remove(field_id);
    }

    /// Checks if a specific contact can see a field.
    pub fn can_see(&self, field_id: &str, contact_id: &str) -> bool {
        match self.get(field_id) {
            FieldVisibility::Everyone => true,
            FieldVisibility::Contacts(allowed) => allowed.contains(contact_id),
            FieldVisibility::Nobody => false,
        }
    }

    /// Returns a list of field IDs that a contact can see.
    pub fn visible_fields(&self, contact_id: &str, all_field_ids: &[&str]) -> Vec<String> {
        all_field_ids
            .iter()
            .filter(|id| self.can_see(id, contact_id))
            .map(|id| id.to_string())
            .collect()
    }
}
