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
        self.rules.get(field_id).unwrap_or(&FieldVisibility::Everyone)
    }

    /// Sets visibility for a field to everyone.
    pub fn set_everyone(&mut self, field_id: &str) {
        self.rules.insert(field_id.to_string(), FieldVisibility::Everyone);
    }

    /// Sets visibility for a field to specific contacts only.
    pub fn set_contacts(&mut self, field_id: &str, contact_ids: HashSet<String>) {
        self.rules.insert(field_id.to_string(), FieldVisibility::Contacts(contact_ids));
    }

    /// Sets visibility for a field to nobody (private).
    pub fn set_nobody(&mut self, field_id: &str) {
        self.rules.insert(field_id.to_string(), FieldVisibility::Nobody);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_is_everyone() {
        let rules = VisibilityRules::new();
        assert_eq!(*rules.get("any_field"), FieldVisibility::Everyone);
    }

    #[test]
    fn test_set_contacts_only() {
        let mut rules = VisibilityRules::new();
        let mut allowed = HashSet::new();
        allowed.insert("alice".to_string());
        allowed.insert("bob".to_string());

        rules.set_contacts("phone", allowed);

        assert!(rules.can_see("phone", "alice"));
        assert!(rules.can_see("phone", "bob"));
        assert!(!rules.can_see("phone", "charlie"));
    }

    #[test]
    fn test_set_nobody() {
        let mut rules = VisibilityRules::new();
        rules.set_nobody("secret_field");

        assert!(!rules.can_see("secret_field", "alice"));
        assert!(!rules.can_see("secret_field", "bob"));
    }

    #[test]
    fn test_visible_fields() {
        let mut rules = VisibilityRules::new();
        rules.set_nobody("private");

        let mut allowed = HashSet::new();
        allowed.insert("alice".to_string());
        rules.set_contacts("restricted", allowed);

        let all_fields = vec!["public", "private", "restricted"];

        let alice_visible = rules.visible_fields("alice", &all_fields);
        assert!(alice_visible.contains(&"public".to_string()));
        assert!(!alice_visible.contains(&"private".to_string()));
        assert!(alice_visible.contains(&"restricted".to_string()));

        let bob_visible = rules.visible_fields("bob", &all_fields);
        assert!(bob_visible.contains(&"public".to_string()));
        assert!(!bob_visible.contains(&"private".to_string()));
        assert!(!bob_visible.contains(&"restricted".to_string()));
    }
}
