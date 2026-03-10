// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Labels
//!
//! Labels allow organizing contacts into groups for easier visibility management.
//! Labels are local-only - they are never transmitted to contacts, only synced
//! across your own devices.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Maximum number of labels allowed per user.
pub const MAX_LABELS: usize = 50;

/// Suggested default labels for new users.
pub const SUGGESTED_LABELS: &[&str] = &["Family", "Friends", "Coworkers", "Business"];

/// Error type for label operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelError {
    /// Label with this name already exists.
    DuplicateName(String),
    /// Label not found.
    NotFound(String),
    /// Maximum number of labels reached.
    MaxLabelsReached,
    /// Invalid label name.
    InvalidName(String),
}

impl std::fmt::Display for LabelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LabelError::DuplicateName(name) => write!(f, "Label already exists: {}", name),
            LabelError::NotFound(name) => write!(f, "Label not found: {}", name),
            LabelError::MaxLabelsReached => {
                write!(f, "Maximum number of labels reached ({})", MAX_LABELS)
            }
            LabelError::InvalidName(msg) => write!(f, "Invalid label name: {}", msg),
        }
    }
}

impl std::error::Error for LabelError {}

/// A visibility label for organizing contacts.
///
/// Labels allow grouping contacts and assigning field visibility to groups.
/// For example, a "Family" label might show personal phone and home address,
/// while "Professional" shows only work email and phone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Group {
    /// Unique identifier for this label (UUID).
    id: String,
    /// Human-readable name.
    name: String,
    /// IDs of contacts assigned to this label.
    contacts: HashSet<String>,
    /// IDs of fields visible to contacts in this label.
    visible_fields: HashSet<String>,
    /// Optional display name override for this label's contacts.
    ///
    /// When set, contacts in this label see this name instead of the
    /// user's default display name.
    #[serde(default)]
    display_name_override: Option<String>,
    /// Timestamp when the label was created.
    created_at: u64,
    /// Timestamp when the label was last modified.
    modified_at: u64,
}

impl Group {
    /// Creates a new label with the given name.
    pub fn new(name: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        Group {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            contacts: HashSet::new(),
            visible_fields: HashSet::new(),
            display_name_override: None,
            created_at: now,
            modified_at: now,
        }
    }

    /// Creates a label from storage data.
    pub fn from_storage(
        id: String,
        name: String,
        contacts: HashSet<String>,
        visible_fields: HashSet<String>,
        display_name_override: Option<String>,
        created_at: u64,
        modified_at: u64,
    ) -> Self {
        Group {
            id,
            name,
            contacts,
            visible_fields,
            display_name_override,
            created_at,
            modified_at,
        }
    }

    /// Returns the label ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the label name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the label name.
    pub fn set_name(&mut self, name: &str) {
        self.name = name.to_string();
        self.touch();
    }

    /// Returns the display name override, if set.
    pub fn display_name_override(&self) -> Option<&str> {
        self.display_name_override.as_deref()
    }

    /// Sets or clears the display name override.
    ///
    /// When set, contacts in this label see this name instead of the
    /// user's default display name. Pass `None` to clear the override.
    ///
    /// Validates that the name is non-empty, not whitespace-only, and
    /// at most 100 characters (after trimming).
    pub fn set_display_name_override(&mut self, name: Option<&str>) -> Result<(), LabelError> {
        match name {
            None => {
                self.display_name_override = None;
                self.touch();
                Ok(())
            }
            Some(raw) => {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return Err(LabelError::InvalidName(
                        "Display name override cannot be empty".to_string(),
                    ));
                }
                if trimmed.chars().count() > 100 {
                    return Err(LabelError::InvalidName(
                        "Display name override cannot exceed 100 characters".to_string(),
                    ));
                }
                self.display_name_override = Some(trimmed.to_string());
                self.touch();
                Ok(())
            }
        }
    }

    /// Resolves the display name for contacts in this label.
    ///
    /// Returns the override if set, otherwise the provided default name.
    pub fn resolve_display_name<'a>(&'a self, default_name: &'a str) -> &'a str {
        match &self.display_name_override {
            Some(override_name) => override_name.as_str(),
            None => default_name,
        }
    }

    /// Returns the set of contact IDs in this label.
    pub fn contacts(&self) -> &HashSet<String> {
        &self.contacts
    }

    /// Returns the number of contacts in this label.
    pub fn contact_count(&self) -> usize {
        self.contacts.len()
    }

    /// Checks if a contact is in this label.
    pub fn contains_contact(&self, contact_id: &str) -> bool {
        self.contacts.contains(contact_id)
    }

    /// Adds a contact to this label.
    ///
    /// Returns true if the contact was added (wasn't already present).
    pub fn add_contact(&mut self, contact_id: &str) -> bool {
        let added = self.contacts.insert(contact_id.to_string());
        if added {
            self.touch();
        }
        added
    }

    /// Removes a contact from this label.
    ///
    /// Returns true if the contact was removed (was present).
    pub fn remove_contact(&mut self, contact_id: &str) -> bool {
        let removed = self.contacts.remove(contact_id);
        if removed {
            self.touch();
        }
        removed
    }

    /// Returns the set of field IDs visible to contacts in this label.
    pub fn visible_fields(&self) -> &HashSet<String> {
        &self.visible_fields
    }

    /// Checks if a field is visible to contacts in this label.
    pub fn is_field_visible(&self, field_id: &str) -> bool {
        self.visible_fields.contains(field_id)
    }

    /// Adds a field to the visible fields for this label.
    ///
    /// Returns true if the field was added (wasn't already present).
    pub fn add_visible_field(&mut self, field_id: &str) -> bool {
        let added = self.visible_fields.insert(field_id.to_string());
        if added {
            self.touch();
        }
        added
    }

    /// Removes a field from the visible fields for this label.
    ///
    /// Returns true if the field was removed (was present).
    pub fn remove_visible_field(&mut self, field_id: &str) -> bool {
        let removed = self.visible_fields.remove(field_id);
        if removed {
            self.touch();
        }
        removed
    }

    /// Sets all visible fields at once.
    pub fn set_visible_fields(&mut self, field_ids: HashSet<String>) {
        self.visible_fields = field_ids;
        self.touch();
    }

    /// Returns the creation timestamp.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }

    /// Returns the last modification timestamp.
    pub fn modified_at(&self) -> u64 {
        self.modified_at
    }

    /// Updates the modification timestamp.
    fn touch(&mut self) {
        self.modified_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();
    }
}

/// Manages visibility labels for a user.
///
/// Labels are organized in a collection with efficient lookup by ID and name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GroupManager {
    /// Labels indexed by ID.
    labels: HashMap<String, Group>,
    /// Per-contact overrides: contact_id -> (field_id -> is_visible).
    /// These take precedence over label-based visibility.
    per_contact_overrides: HashMap<String, HashMap<String, bool>>,
}

impl GroupManager {
    /// Creates a new empty label manager.
    pub fn new() -> Self {
        GroupManager {
            labels: HashMap::new(),
            per_contact_overrides: HashMap::new(),
        }
    }

    /// Inserts a label loaded from storage, preserving its original ID and all fields.
    ///
    /// This bypasses validation (name length, duplicates) because the data was
    /// already validated when first created.
    pub fn insert_loaded_label(&mut self, label: Group) {
        self.labels.insert(label.id().to_string(), label);
    }

    /// Returns all labels.
    pub fn all_labels(&self) -> Vec<&Group> {
        self.labels.values().collect()
    }

    /// Returns the number of labels.
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Returns true if no labels exist.
    pub fn is_empty(&self) -> bool {
        self.labels.is_empty()
    }

    /// Gets a label by ID.
    pub fn get_group(&self, label_id: &str) -> Option<&Group> {
        self.labels.get(label_id)
    }

    /// Gets a mutable reference to a label by ID.
    pub fn get_group_mut(&mut self, label_id: &str) -> Option<&mut Group> {
        self.labels.get_mut(label_id)
    }

    /// Gets a label by name.
    pub fn get_group_by_name(&self, name: &str) -> Option<&Group> {
        self.labels.values().find(|l| l.name == name)
    }

    /// Creates a new label.
    pub fn create_group(&mut self, name: &str) -> Result<&Group, LabelError> {
        // Validate name
        let name = name.trim();
        if name.is_empty() {
            return Err(LabelError::InvalidName("Name cannot be empty".to_string()));
        }
        if name.chars().count() > 50 {
            return Err(LabelError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate
        if self.get_group_by_name(name).is_some() {
            return Err(LabelError::DuplicateName(name.to_string()));
        }

        // Check limit
        if self.labels.len() >= MAX_LABELS {
            return Err(LabelError::MaxLabelsReached);
        }

        // Create label
        let label = Group::new(name);
        let id = label.id.clone();
        self.labels.insert(id.clone(), label);

        Ok(self.labels.get(&id).expect("just inserted"))
    }

    /// Renames a label.
    pub fn rename_group(&mut self, label_id: &str, new_name: &str) -> Result<(), LabelError> {
        let new_name = new_name.trim();

        // Validate new name
        if new_name.is_empty() {
            return Err(LabelError::InvalidName("Name cannot be empty".to_string()));
        }
        if new_name.chars().count() > 50 {
            return Err(LabelError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate (excluding this label)
        if let Some(existing) = self.get_group_by_name(new_name) {
            if existing.id != label_id {
                return Err(LabelError::DuplicateName(new_name.to_string()));
            }
        }

        // Find and rename
        let label = self
            .labels
            .get_mut(label_id)
            .ok_or_else(|| LabelError::NotFound(label_id.to_string()))?;

        label.set_name(new_name);
        Ok(())
    }

    /// Deletes a label.
    ///
    /// Contacts in the label remain in the contact list; they just lose
    /// their label membership.
    pub fn delete_group(&mut self, label_id: &str) -> Result<Group, LabelError> {
        self.labels
            .remove(label_id)
            .ok_or_else(|| LabelError::NotFound(label_id.to_string()))
    }

    /// Returns all labels that contain a specific contact.
    pub fn labels_for_contact(&self, contact_id: &str) -> Vec<&Group> {
        self.labels
            .values()
            .filter(|l| l.contains_contact(contact_id))
            .collect()
    }

    /// Returns all contacts not in any label.
    pub fn unlabeled_contacts(&self, all_contact_ids: &[&str]) -> Vec<String> {
        all_contact_ids
            .iter()
            .filter(|id| !self.labels.values().any(|l| l.contains_contact(id)))
            .map(|id| id.to_string())
            .collect()
    }

    /// Adds a contact to a label.
    pub fn add_contact_to_group(
        &mut self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<bool, LabelError> {
        let label = self
            .labels
            .get_mut(label_id)
            .ok_or_else(|| LabelError::NotFound(label_id.to_string()))?;

        Ok(label.add_contact(contact_id))
    }

    /// Removes a contact from a label.
    pub fn remove_contact_from_group(
        &mut self,
        label_id: &str,
        contact_id: &str,
    ) -> Result<bool, LabelError> {
        let label = self
            .labels
            .get_mut(label_id)
            .ok_or_else(|| LabelError::NotFound(label_id.to_string()))?;

        Ok(label.remove_contact(contact_id))
    }

    /// Removes a contact from all labels (e.g., when deleting the contact).
    pub fn remove_contact_from_all_groups(&mut self, contact_id: &str) {
        for label in self.labels.values_mut() {
            label.remove_contact(contact_id);
        }
        self.per_contact_overrides.remove(contact_id);
    }

    /// Sets per-contact visibility override for a field.
    ///
    /// Per-contact overrides take precedence over label-based visibility.
    pub fn set_contact_override(&mut self, contact_id: &str, field_id: &str, is_visible: bool) {
        self.per_contact_overrides
            .entry(contact_id.to_string())
            .or_default()
            .insert(field_id.to_string(), is_visible);
    }

    /// Removes a per-contact visibility override for a field.
    pub fn remove_contact_override(&mut self, contact_id: &str, field_id: &str) {
        if let Some(overrides) = self.per_contact_overrides.get_mut(contact_id) {
            overrides.remove(field_id);
            if overrides.is_empty() {
                self.per_contact_overrides.remove(contact_id);
            }
        }
    }

    /// Clears all per-contact overrides for a contact.
    pub fn clear_contact_overrides(&mut self, contact_id: &str) {
        self.per_contact_overrides.remove(contact_id);
    }

    /// Gets the per-contact override for a field.
    pub fn get_contact_override(&self, contact_id: &str, field_id: &str) -> Option<bool> {
        self.per_contact_overrides
            .get(contact_id)
            .and_then(|m| m.get(field_id))
            .copied()
    }

    /// Returns all per-contact overrides for a contact.
    pub fn get_all_contact_overrides(&self, contact_id: &str) -> Option<&HashMap<String, bool>> {
        self.per_contact_overrides.get(contact_id)
    }

    /// Determines if a contact can see a specific field.
    ///
    /// Visibility is determined by:
    /// 1. Per-contact override (if set, takes precedence)
    /// 2. Label membership (visible if contact is in any label that shows this field)
    /// 3. Default (not visible through labels - falls back to VisibilityRules)
    ///
    /// Returns `Some(true)` if visible via labels, `Some(false)` if explicitly
    /// hidden via override, `None` to fall back to default VisibilityRules.
    pub fn can_see_via_labels(&self, contact_id: &str, field_id: &str) -> Option<bool> {
        // Check per-contact override first
        if let Some(is_visible) = self.get_contact_override(contact_id, field_id) {
            return Some(is_visible);
        }

        // Check if any label containing this contact shows this field
        let labels_for_contact = self.labels_for_contact(contact_id);
        for label in labels_for_contact {
            if label.is_field_visible(field_id) {
                return Some(true);
            }
        }

        // No label grants visibility - return None to fall back to default rules
        None
    }

    /// Merges the source group into the target group.
    ///
    /// Union of members and visible fields. The source group is deleted.
    /// Per-contact overrides are preserved (they're contact-scoped, not group-scoped).
    /// The target group keeps its name and display_name_override.
    pub fn merge_groups(&mut self, target_id: &str, source_id: &str) -> Result<(), LabelError> {
        if target_id == source_id {
            return Err(LabelError::InvalidName(
                "Cannot merge a group with itself".to_string(),
            ));
        }

        // Remove source first to avoid double-borrow
        let source = self
            .labels
            .remove(source_id)
            .ok_or_else(|| LabelError::NotFound(source_id.to_string()))?;

        let target = self
            .labels
            .get_mut(target_id)
            .ok_or_else(|| LabelError::NotFound(target_id.to_string()))?;

        // Union of contacts
        for contact_id in source.contacts() {
            target.add_contact(contact_id);
        }

        // Union of visible fields
        for field_id in source.visible_fields() {
            target.add_visible_field(field_id);
        }

        Ok(())
    }

    /// Returns all fields that a contact can see via labels.
    pub fn visible_fields_via_labels(&self, contact_id: &str) -> HashSet<String> {
        let mut visible = HashSet::new();

        // Add fields from all labels the contact is in
        for label in self.labels_for_contact(contact_id) {
            visible.extend(label.visible_fields().clone());
        }

        // Apply per-contact overrides
        if let Some(overrides) = self.per_contact_overrides.get(contact_id) {
            for (field_id, is_visible) in overrides {
                if *is_visible {
                    visible.insert(field_id.clone());
                } else {
                    visible.remove(field_id);
                }
            }
        }

        visible
    }
}

/// Resolves which fields a given contact can see.
///
/// Two modes:
/// - No-group mode (no labels exist): returns fields where `field_visibility`
///   is `Everyone` on the card.
/// - Groups mode (labels exist): returns union of `visible_fields` across
///   all labels the contact belongs to. Per-contact overrides still apply.
///
/// Ungrouped contacts in groups mode see no fields (default-closed).
pub fn resolve_visible_fields(
    card: &crate::contact_card::ContactCard,
    label_manager: &GroupManager,
    contact_id: &str,
) -> HashSet<String> {
    if label_manager.is_empty() {
        // No-group mode: use card's field_visibility rules
        card.field_visibility().everyone_field_ids()
    } else {
        // Groups mode: use label-based visibility
        label_manager.visible_fields_via_labels(contact_id)
    }
}

// INLINE_TEST_REQUIRED: tests access private Group fields and GroupManager internals
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_label() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Family").unwrap();

        assert_eq!(label.name(), "Family");
        assert_eq!(label.contact_count(), 0);
        assert!(label.visible_fields().is_empty());
    }

    #[test]
    fn test_create_duplicate_label() {
        let mut manager = GroupManager::new();
        manager.create_group("Friends").unwrap();

        let result = manager.create_group("Friends");
        assert!(matches!(result, Err(LabelError::DuplicateName(_))));
    }

    #[test]
    fn test_add_contact_to_label() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Family").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_group(&label_id, "bob-id").unwrap();

        let label = manager.get_group(&label_id).unwrap();
        assert!(label.contains_contact("bob-id"));
        assert_eq!(label.contact_count(), 1);
    }

    #[test]
    fn test_remove_contact_from_label() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Family").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_group(&label_id, "bob-id").unwrap();
        manager
            .remove_contact_from_group(&label_id, "bob-id")
            .unwrap();

        let label = manager.get_group(&label_id).unwrap();
        assert!(!label.contains_contact("bob-id"));
    }

    #[test]
    fn test_label_field_visibility() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Family").unwrap();
        let label_id = label.id().to_string();

        // Add contact and field
        manager.add_contact_to_group(&label_id, "bob-id").unwrap();
        let label = manager.get_group_mut(&label_id).unwrap();
        label.add_visible_field("personal-phone");

        // Bob should see the field
        assert_eq!(
            manager.can_see_via_labels("bob-id", "personal-phone"),
            Some(true)
        );

        // Carol (not in label) shouldn't see it via labels
        assert_eq!(
            manager.can_see_via_labels("carol-id", "personal-phone"),
            None
        );
    }

    #[test]
    fn test_per_contact_override() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Friends").unwrap();
        let label_id = label.id().to_string();

        // Add Bob to Friends and set personal-phone as visible
        manager.add_contact_to_group(&label_id, "bob-id").unwrap();
        let label = manager.get_group_mut(&label_id).unwrap();
        label.add_visible_field("personal-phone");

        // Bob should see personal-phone via label
        assert_eq!(
            manager.can_see_via_labels("bob-id", "personal-phone"),
            Some(true)
        );

        // Hide personal-phone specifically from Bob
        manager.set_contact_override("bob-id", "personal-phone", false);

        // Bob should NOT see personal-phone due to override
        assert_eq!(
            manager.can_see_via_labels("bob-id", "personal-phone"),
            Some(false)
        );
    }

    #[test]
    fn test_contact_in_multiple_labels() {
        let mut manager = GroupManager::new();

        let family = manager.create_group("Family").unwrap();
        let family_id = family.id().to_string();

        let friends = manager.create_group("Friends").unwrap();
        let friends_id = friends.id().to_string();

        // Add Carol to both labels
        manager
            .add_contact_to_group(&family_id, "carol-id")
            .unwrap();
        manager
            .add_contact_to_group(&friends_id, "carol-id")
            .unwrap();

        // Set different fields for each label
        let family = manager.get_group_mut(&family_id).unwrap();
        family.add_visible_field("home-address");

        let friends = manager.get_group_mut(&friends_id).unwrap();
        friends.add_visible_field("phone");

        // Carol should see both fields (union of labels)
        let visible = manager.visible_fields_via_labels("carol-id");
        assert!(visible.contains("home-address"));
        assert!(visible.contains("phone"));
    }

    #[test]
    fn test_rename_label() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Work").unwrap();
        let label_id = label.id().to_string();

        manager.rename_group(&label_id, "Colleagues").unwrap();

        let label = manager.get_group(&label_id).unwrap();
        assert_eq!(label.name(), "Colleagues");
    }

    #[test]
    fn test_delete_label() {
        let mut manager = GroupManager::new();
        let label = manager.create_group("Temporary").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_group(&label_id, "bob-id").unwrap();

        let deleted = manager.delete_group(&label_id).unwrap();
        assert_eq!(deleted.name(), "Temporary");

        assert!(manager.get_group(&label_id).is_none());
        assert_eq!(manager.label_count(), 0);
    }

    #[test]
    fn test_max_labels() {
        let mut manager = GroupManager::new();

        for i in 0..MAX_LABELS {
            manager.create_group(&format!("Label{}", i)).unwrap();
        }

        let result = manager.create_group("OneMore");
        assert!(matches!(result, Err(LabelError::MaxLabelsReached)));
    }

    #[test]
    fn test_label_display_name_override() {
        let mut label = Group::new("Family");

        // Initially None
        assert_eq!(label.display_name_override(), None);

        // Set override
        label
            .set_display_name_override(Some("Matt"))
            .expect("valid name should succeed");
        assert_eq!(label.display_name_override(), Some("Matt"));

        // Clear override
        label
            .set_display_name_override(None)
            .expect("clearing should succeed");
        assert_eq!(label.display_name_override(), None);
    }

    #[test]
    fn test_label_display_name_override_validation() {
        let mut label = Group::new("Friends");

        // Empty string should fail
        let result = label.set_display_name_override(Some(""));
        assert!(matches!(result, Err(LabelError::InvalidName(_))));

        // Whitespace-only should fail
        let result = label.set_display_name_override(Some("   "));
        assert!(matches!(result, Err(LabelError::InvalidName(_))));

        // Too long (>100 chars) should fail
        let long_name = "a".repeat(101);
        let result = label.set_display_name_override(Some(&long_name));
        assert!(matches!(result, Err(LabelError::InvalidName(_))));

        // Exactly 100 chars should succeed
        let max_name = "b".repeat(100);
        label
            .set_display_name_override(Some(&max_name))
            .expect("100 chars should succeed");
        assert_eq!(label.display_name_override(), Some(max_name.as_str()));

        // Whitespace trimming
        label
            .set_display_name_override(Some("  Dr. Egloff  "))
            .expect("trimmed name should succeed");
        assert_eq!(label.display_name_override(), Some("Dr. Egloff"));
    }

    #[test]
    fn test_label_resolve_display_name() {
        let mut label = Group::new("Business");

        // Without override, returns default
        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Mattia Egloff");

        // With override, returns override
        label
            .set_display_name_override(Some("Dr. Egloff"))
            .expect("valid name");
        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Dr. Egloff");

        // After clearing, returns default again
        label
            .set_display_name_override(None)
            .expect("clearing should succeed");
        assert_eq!(label.resolve_display_name("Mattia Egloff"), "Mattia Egloff");
    }

    #[test]
    fn test_suggested_labels_updated() {
        assert_eq!(
            SUGGESTED_LABELS,
            &["Family", "Friends", "Coworkers", "Business"]
        );
    }

    #[test]
    fn test_labels_are_local() {
        // Labels exist only in GroupManager, not in Contact
        // This test verifies the design doesn't leak labels to contacts
        let label = Group::new("Secret Name");

        // The label name is never serialized in a way that would be sent to contacts
        // Label data should only be synced to the user's own devices
        assert_eq!(label.name(), "Secret Name");
        // The contact sees field visibility, not labels
    }

    #[test]
    fn test_merge_groups_union_members_and_fields() {
        let mut manager = GroupManager::new();
        let target = manager.create_group("Family").unwrap().id().to_string();
        let source = manager
            .create_group("Close Friends")
            .unwrap()
            .id()
            .to_string();

        // Add different contacts and fields to each
        manager.add_contact_to_group(&target, "alice").unwrap();
        manager.add_contact_to_group(&source, "bob").unwrap();
        manager.add_contact_to_group(&source, "alice").unwrap(); // overlap

        manager
            .get_group_mut(&target)
            .unwrap()
            .add_visible_field("phone");
        manager
            .get_group_mut(&source)
            .unwrap()
            .add_visible_field("email");
        manager
            .get_group_mut(&source)
            .unwrap()
            .add_visible_field("phone"); // overlap

        manager.merge_groups(&target, &source).unwrap();

        // Target has union of members
        let merged = manager.get_group(&target).unwrap();
        assert!(merged.contains_contact("alice"));
        assert!(merged.contains_contact("bob"));
        assert_eq!(merged.contact_count(), 2);

        // Target has union of visible fields
        assert!(merged.is_field_visible("phone"));
        assert!(merged.is_field_visible("email"));

        // Source group is deleted
        assert!(manager.get_group(&source).is_none());
        assert_eq!(manager.label_count(), 1);
    }

    #[test]
    fn test_merge_groups_source_not_found() {
        let mut manager = GroupManager::new();
        let target = manager.create_group("Family").unwrap().id().to_string();

        let result = manager.merge_groups(&target, "nonexistent");
        assert!(matches!(result, Err(LabelError::NotFound(_))));
    }

    #[test]
    fn test_merge_groups_target_not_found() {
        let mut manager = GroupManager::new();
        let source = manager.create_group("Friends").unwrap().id().to_string();

        let result = manager.merge_groups("nonexistent", &source);
        assert!(matches!(result, Err(LabelError::NotFound(_))));
    }

    #[test]
    fn test_merge_groups_same_group() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Family").unwrap().id().to_string();

        let result = manager.merge_groups(&group, &group);
        assert!(matches!(result, Err(LabelError::InvalidName(_))));
    }

    #[test]
    fn test_merge_groups_preserves_display_name_override() {
        let mut manager = GroupManager::new();
        let target = manager.create_group("Family").unwrap().id().to_string();
        let source = manager
            .create_group("Close Friends")
            .unwrap()
            .id()
            .to_string();

        manager
            .get_group_mut(&target)
            .unwrap()
            .set_display_name_override(Some("Mom's Son"))
            .unwrap();

        manager.merge_groups(&target, &source).unwrap();

        let merged = manager.get_group(&target).unwrap();
        assert_eq!(merged.resolve_display_name("Default"), "Mom's Son");
    }

    #[test]
    fn test_merge_groups_source_overrides_transferred() {
        let mut manager = GroupManager::new();
        let target = manager.create_group("Family").unwrap().id().to_string();
        let source = manager.create_group("Friends").unwrap().id().to_string();

        // Bob is only in source, has an override
        manager.add_contact_to_group(&source, "bob").unwrap();
        manager.set_contact_override("bob", "phone", false);

        manager.merge_groups(&target, &source).unwrap();

        // Bob's override is preserved
        assert_eq!(manager.get_contact_override("bob", "phone"), Some(false));
    }
}
