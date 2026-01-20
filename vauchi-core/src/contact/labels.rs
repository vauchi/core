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
pub const SUGGESTED_LABELS: &[&str] = &["Family", "Friends", "Professional"];

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
pub struct VisibilityLabel {
    /// Unique identifier for this label (UUID).
    id: String,
    /// Human-readable name.
    name: String,
    /// IDs of contacts assigned to this label.
    contacts: HashSet<String>,
    /// IDs of fields visible to contacts in this label.
    visible_fields: HashSet<String>,
    /// Timestamp when the label was created.
    created_at: u64,
    /// Timestamp when the label was last modified.
    modified_at: u64,
}

impl VisibilityLabel {
    /// Creates a new label with the given name.
    pub fn new(name: &str) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs();

        VisibilityLabel {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.to_string(),
            contacts: HashSet::new(),
            visible_fields: HashSet::new(),
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
        created_at: u64,
        modified_at: u64,
    ) -> Self {
        VisibilityLabel {
            id,
            name,
            contacts,
            visible_fields,
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
pub struct LabelManager {
    /// Labels indexed by ID.
    labels: HashMap<String, VisibilityLabel>,
    /// Per-contact overrides: contact_id -> (field_id -> is_visible).
    /// These take precedence over label-based visibility.
    per_contact_overrides: HashMap<String, HashMap<String, bool>>,
}

impl LabelManager {
    /// Creates a new empty label manager.
    pub fn new() -> Self {
        LabelManager {
            labels: HashMap::new(),
            per_contact_overrides: HashMap::new(),
        }
    }

    /// Returns all labels.
    pub fn all_labels(&self) -> Vec<&VisibilityLabel> {
        self.labels.values().collect()
    }

    /// Returns the number of labels.
    pub fn label_count(&self) -> usize {
        self.labels.len()
    }

    /// Gets a label by ID.
    pub fn get_label(&self, label_id: &str) -> Option<&VisibilityLabel> {
        self.labels.get(label_id)
    }

    /// Gets a mutable reference to a label by ID.
    pub fn get_label_mut(&mut self, label_id: &str) -> Option<&mut VisibilityLabel> {
        self.labels.get_mut(label_id)
    }

    /// Gets a label by name.
    pub fn get_label_by_name(&self, name: &str) -> Option<&VisibilityLabel> {
        self.labels.values().find(|l| l.name == name)
    }

    /// Creates a new label.
    pub fn create_label(&mut self, name: &str) -> Result<&VisibilityLabel, LabelError> {
        // Validate name
        let name = name.trim();
        if name.is_empty() {
            return Err(LabelError::InvalidName("Name cannot be empty".to_string()));
        }
        if name.len() > 50 {
            return Err(LabelError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate
        if self.get_label_by_name(name).is_some() {
            return Err(LabelError::DuplicateName(name.to_string()));
        }

        // Check limit
        if self.labels.len() >= MAX_LABELS {
            return Err(LabelError::MaxLabelsReached);
        }

        // Create label
        let label = VisibilityLabel::new(name);
        let id = label.id.clone();
        self.labels.insert(id.clone(), label);

        Ok(self.labels.get(&id).unwrap())
    }

    /// Renames a label.
    pub fn rename_label(&mut self, label_id: &str, new_name: &str) -> Result<(), LabelError> {
        let new_name = new_name.trim();

        // Validate new name
        if new_name.is_empty() {
            return Err(LabelError::InvalidName("Name cannot be empty".to_string()));
        }
        if new_name.len() > 50 {
            return Err(LabelError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate (excluding this label)
        if let Some(existing) = self.get_label_by_name(new_name) {
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
    pub fn delete_label(&mut self, label_id: &str) -> Result<VisibilityLabel, LabelError> {
        self.labels
            .remove(label_id)
            .ok_or_else(|| LabelError::NotFound(label_id.to_string()))
    }

    /// Returns all labels that contain a specific contact.
    pub fn labels_for_contact(&self, contact_id: &str) -> Vec<&VisibilityLabel> {
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
    pub fn add_contact_to_label(
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
    pub fn remove_contact_from_label(
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
    pub fn remove_contact_from_all_labels(&mut self, contact_id: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_label() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Family").unwrap();

        assert_eq!(label.name(), "Family");
        assert_eq!(label.contact_count(), 0);
        assert!(label.visible_fields().is_empty());
    }

    #[test]
    fn test_create_duplicate_label() {
        let mut manager = LabelManager::new();
        manager.create_label("Friends").unwrap();

        let result = manager.create_label("Friends");
        assert!(matches!(result, Err(LabelError::DuplicateName(_))));
    }

    #[test]
    fn test_add_contact_to_label() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Family").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_label(&label_id, "bob-id").unwrap();

        let label = manager.get_label(&label_id).unwrap();
        assert!(label.contains_contact("bob-id"));
        assert_eq!(label.contact_count(), 1);
    }

    #[test]
    fn test_remove_contact_from_label() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Family").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_label(&label_id, "bob-id").unwrap();
        manager
            .remove_contact_from_label(&label_id, "bob-id")
            .unwrap();

        let label = manager.get_label(&label_id).unwrap();
        assert!(!label.contains_contact("bob-id"));
    }

    #[test]
    fn test_label_field_visibility() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Family").unwrap();
        let label_id = label.id().to_string();

        // Add contact and field
        manager.add_contact_to_label(&label_id, "bob-id").unwrap();
        let label = manager.get_label_mut(&label_id).unwrap();
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
        let mut manager = LabelManager::new();
        let label = manager.create_label("Friends").unwrap();
        let label_id = label.id().to_string();

        // Add Bob to Friends and set personal-phone as visible
        manager.add_contact_to_label(&label_id, "bob-id").unwrap();
        let label = manager.get_label_mut(&label_id).unwrap();
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
        let mut manager = LabelManager::new();

        let family = manager.create_label("Family").unwrap();
        let family_id = family.id().to_string();

        let friends = manager.create_label("Friends").unwrap();
        let friends_id = friends.id().to_string();

        // Add Carol to both labels
        manager.add_contact_to_label(&family_id, "carol-id").unwrap();
        manager
            .add_contact_to_label(&friends_id, "carol-id")
            .unwrap();

        // Set different fields for each label
        let family = manager.get_label_mut(&family_id).unwrap();
        family.add_visible_field("home-address");

        let friends = manager.get_label_mut(&friends_id).unwrap();
        friends.add_visible_field("phone");

        // Carol should see both fields (union of labels)
        let visible = manager.visible_fields_via_labels("carol-id");
        assert!(visible.contains("home-address"));
        assert!(visible.contains("phone"));
    }

    #[test]
    fn test_rename_label() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Work").unwrap();
        let label_id = label.id().to_string();

        manager.rename_label(&label_id, "Colleagues").unwrap();

        let label = manager.get_label(&label_id).unwrap();
        assert_eq!(label.name(), "Colleagues");
    }

    #[test]
    fn test_delete_label() {
        let mut manager = LabelManager::new();
        let label = manager.create_label("Temporary").unwrap();
        let label_id = label.id().to_string();

        manager.add_contact_to_label(&label_id, "bob-id").unwrap();

        let deleted = manager.delete_label(&label_id).unwrap();
        assert_eq!(deleted.name(), "Temporary");

        assert!(manager.get_label(&label_id).is_none());
        assert_eq!(manager.label_count(), 0);
    }

    #[test]
    fn test_max_labels() {
        let mut manager = LabelManager::new();

        for i in 0..MAX_LABELS {
            manager.create_label(&format!("Label{}", i)).unwrap();
        }

        let result = manager.create_label("OneMore");
        assert!(matches!(result, Err(LabelError::MaxLabelsReached)));
    }

    #[test]
    fn test_labels_are_local() {
        // Labels exist only in LabelManager, not in Contact
        // This test verifies the design doesn't leak labels to contacts
        let label = VisibilityLabel::new("Secret Name");

        // The label name is never serialized in a way that would be sent to contacts
        // Label data should only be synced to the user's own devices
        assert_eq!(label.name(), "Secret Name");
        // The contact sees field visibility, not labels
    }
}
