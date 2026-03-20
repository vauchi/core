// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Group manager: assignment, visibility resolution, and merge logic.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::{Group, GroupError, MAX_LABELS};

/// Manages contact groups for a user.
///
/// Groups are organized in a collection with efficient lookup by ID and name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct GroupManager {
    /// Groups indexed by ID.
    #[serde(alias = "labels")]
    groups: HashMap<String, Group>,
    /// Per-contact overrides: contact_id -> (field_id -> is_visible).
    /// These take precedence over group-based visibility.
    per_contact_overrides: HashMap<String, HashMap<String, bool>>,
}

impl GroupManager {
    /// Creates a new empty group manager.
    pub fn new() -> Self {
        GroupManager {
            groups: HashMap::new(),
            per_contact_overrides: HashMap::new(),
        }
    }

    /// Inserts a group loaded from storage, preserving its original ID and all fields.
    ///
    /// This bypasses validation (name length, duplicates) because the data was
    /// already validated when first created.
    pub fn insert_loaded_group(&mut self, group: Group) {
        self.groups.insert(group.id().to_string(), group);
    }

    /// Returns all groups.
    pub fn all_groups(&self) -> Vec<&Group> {
        self.groups.values().collect()
    }

    /// Returns the number of groups.
    pub fn group_count(&self) -> usize {
        self.groups.len()
    }

    /// Returns true if no groups exist.
    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    /// Gets a group by ID.
    pub fn get_group(&self, group_id: &str) -> Option<&Group> {
        self.groups.get(group_id)
    }

    /// Gets a mutable reference to a group by ID.
    pub fn get_group_mut(&mut self, group_id: &str) -> Option<&mut Group> {
        self.groups.get_mut(group_id)
    }

    /// Gets a group by name.
    pub fn get_group_by_name(&self, name: &str) -> Option<&Group> {
        self.groups.values().find(|l| l.name() == name)
    }

    /// Creates a new group.
    pub fn create_group(&mut self, name: &str) -> Result<&Group, GroupError> {
        // Validate name
        let name = name.trim();
        if name.is_empty() {
            return Err(GroupError::InvalidName("Name cannot be empty".to_string()));
        }
        if name.chars().count() > 50 {
            return Err(GroupError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate
        if self.get_group_by_name(name).is_some() {
            return Err(GroupError::DuplicateName(name.to_string()));
        }

        // Check limit
        if self.groups.len() >= MAX_LABELS {
            return Err(GroupError::MaxLabelsReached);
        }

        // Create group
        let group = Group::new(name);
        let id = group.id().to_string();
        self.groups.insert(id.clone(), group);

        Ok(self.groups.get(&id).expect("just inserted"))
    }

    /// Renames a group.
    pub fn rename_group(&mut self, group_id: &str, new_name: &str) -> Result<(), GroupError> {
        let new_name = new_name.trim();

        // Validate new name
        if new_name.is_empty() {
            return Err(GroupError::InvalidName("Name cannot be empty".to_string()));
        }
        if new_name.chars().count() > 50 {
            return Err(GroupError::InvalidName(
                "Name cannot exceed 50 characters".to_string(),
            ));
        }

        // Check for duplicate (excluding this group)
        if let Some(existing) = self.get_group_by_name(new_name)
            && existing.id() != group_id
        {
            return Err(GroupError::DuplicateName(new_name.to_string()));
        }

        // Find and rename
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| GroupError::NotFound(group_id.to_string()))?;

        group.set_name(new_name);
        Ok(())
    }

    /// Deletes a group.
    ///
    /// Contacts in the group remain in the contact list; they just lose
    /// their group membership.
    pub fn delete_group(&mut self, group_id: &str) -> Result<Group, GroupError> {
        self.groups
            .remove(group_id)
            .ok_or_else(|| GroupError::NotFound(group_id.to_string()))
    }

    /// Returns all groups that contain a specific contact.
    pub fn groups_for_contact(&self, contact_id: &str) -> Vec<&Group> {
        self.groups
            .values()
            .filter(|l| l.contains_contact(contact_id))
            .collect()
    }

    /// Returns all contacts not in any group.
    pub fn ungrouped_contacts(&self, all_contact_ids: &[&str]) -> Vec<String> {
        all_contact_ids
            .iter()
            .filter(|id| !self.groups.values().any(|l| l.contains_contact(id)))
            .map(|id| id.to_string())
            .collect()
    }

    /// Adds a contact to a group.
    pub fn add_contact_to_group(
        &mut self,
        group_id: &str,
        contact_id: &str,
    ) -> Result<bool, GroupError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| GroupError::NotFound(group_id.to_string()))?;

        Ok(group.add_contact(contact_id))
    }

    /// Removes a contact from a group.
    pub fn remove_contact_from_group(
        &mut self,
        group_id: &str,
        contact_id: &str,
    ) -> Result<bool, GroupError> {
        let group = self
            .groups
            .get_mut(group_id)
            .ok_or_else(|| GroupError::NotFound(group_id.to_string()))?;

        Ok(group.remove_contact(contact_id))
    }

    /// Removes a contact from all groups (e.g., when deleting the contact).
    pub fn remove_contact_from_all_groups(&mut self, contact_id: &str) {
        for group in self.groups.values_mut() {
            group.remove_contact(contact_id);
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
    /// 2. Group membership (visible if contact is in any group that shows this field)
    /// 3. Default (not visible through groups - falls back to VisibilityRules)
    ///
    /// Returns `Some(true)` if visible via groups, `Some(false)` if explicitly
    /// hidden via override, `None` to fall back to default VisibilityRules.
    pub fn can_see_via_labels(&self, contact_id: &str, field_id: &str) -> Option<bool> {
        // Check per-contact override first
        if let Some(is_visible) = self.get_contact_override(contact_id, field_id) {
            return Some(is_visible);
        }

        // Check if any group containing this contact shows this field
        let groups = self.groups_for_contact(contact_id);
        for group in groups {
            if group.is_field_visible(field_id) {
                return Some(true);
            }
        }

        // No group grants visibility - return None to fall back to default rules
        None
    }

    /// Merges the source group into the target group.
    ///
    /// Union of members and visible fields. The source group is deleted.
    /// Per-contact overrides are preserved (they're contact-scoped, not group-scoped).
    /// The target group keeps its name and display_name_override.
    pub fn merge_groups(&mut self, target_id: &str, source_id: &str) -> Result<(), GroupError> {
        if target_id == source_id {
            return Err(GroupError::InvalidName(
                "Cannot merge a group with itself".to_string(),
            ));
        }

        // Validate target exists BEFORE removing source to prevent data loss
        if !self.groups.contains_key(target_id) {
            return Err(GroupError::NotFound(target_id.to_string()));
        }

        // Remove source (safe now — target is known to exist)
        let source = self
            .groups
            .remove(source_id)
            .ok_or_else(|| GroupError::NotFound(source_id.to_string()))?;

        let target = self
            .groups
            .get_mut(target_id)
            .expect("target existence verified above");

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

    /// Returns all fields that a contact can see via groups.
    pub fn visible_fields_via_labels(&self, contact_id: &str) -> HashSet<String> {
        let mut visible = HashSet::new();

        // Add fields from all groups the contact is in
        for group in self.groups_for_contact(contact_id) {
            visible.extend(group.visible_fields().clone());
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

// INLINE_TEST_REQUIRED: tests access private GroupManager internals
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
        assert!(matches!(result, Err(GroupError::DuplicateName(_))));
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
        assert_eq!(manager.group_count(), 0);
    }

    #[test]
    fn test_max_labels() {
        let mut manager = GroupManager::new();

        for i in 0..MAX_LABELS {
            manager.create_group(&format!("Label{}", i)).unwrap();
        }

        let result = manager.create_group("OneMore");
        assert!(matches!(result, Err(GroupError::MaxLabelsReached)));
    }

    #[test]
    fn test_suggested_labels_updated() {
        assert_eq!(
            super::super::SUGGESTED_LABELS,
            &["Family", "Friends", "Coworkers", "Business"]
        );
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
        assert_eq!(manager.group_count(), 1);
    }

    #[test]
    fn test_merge_groups_source_not_found() {
        let mut manager = GroupManager::new();
        let target = manager.create_group("Family").unwrap().id().to_string();

        let result = manager.merge_groups(&target, "nonexistent");
        assert!(matches!(result, Err(GroupError::NotFound(_))));
    }

    #[test]
    fn test_merge_groups_target_not_found() {
        let mut manager = GroupManager::new();
        let source = manager.create_group("Friends").unwrap().id().to_string();

        let result = manager.merge_groups("nonexistent", &source);
        assert!(matches!(result, Err(GroupError::NotFound(_))));
        assert!(
            manager.get_group(&source).is_some(),
            "source must survive a failed merge"
        );
    }

    #[test]
    fn test_merge_groups_same_group() {
        let mut manager = GroupManager::new();
        let group = manager.create_group("Family").unwrap().id().to_string();

        let result = manager.merge_groups(&group, &group);
        assert!(matches!(result, Err(GroupError::InvalidName(_))));
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
