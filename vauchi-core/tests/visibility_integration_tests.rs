// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    contact::GroupManager,
    contact_card::{ContactCard, ContactField, FieldType},
};

#[cfg(test)]
mod visibility_integration_tests {
    use super::*;

    fn create_test_label_manager() -> GroupManager {
        GroupManager::new()
    }

    fn create_test_contact_card() -> ContactCard {
        let mut card = ContactCard::new("Test Contact");
        card.add_field(ContactField::new(
            FieldType::Email,
            "Email",
            "test@example.com",
        ))
        .unwrap();
        card.add_field(ContactField::new(FieldType::Phone, "Phone", "+1234567890"))
            .unwrap();
        card
    }

    // @scenario: visibility_control.feature:Create a visibility group
    #[test]
    fn test_label_creation() {
        let mut manager = create_test_label_manager();
        let label = manager.create_group("Work Contacts").unwrap();
        assert_eq!(label.name(), "Work Contacts");
        assert_eq!(manager.group_count(), 1);
    }

    // @scenario: visibility_control.feature:Apply visibility group to a field
    #[test]
    fn test_field_label_association() {
        let mut manager = create_test_label_manager();

        // Create a label
        let label_id = manager
            .create_group("Work Contacts")
            .unwrap()
            .id()
            .to_string();

        // Create card and get field ID
        let card = create_test_contact_card();
        let field_id = card.fields()[0].id();

        // Associate field with label
        let label = manager.get_group_mut(&label_id).unwrap();
        label.add_visible_field(field_id);

        // Verify association
        let label = manager.get_group(&label_id).unwrap();
        assert!(label.is_field_visible(field_id));
    }

    // @scenario: visibility_control.feature:Apply visibility group to a field
    #[test]
    fn test_multiple_label_field_visibility() {
        let mut manager = create_test_label_manager();

        // Create multiple labels
        let work_id = manager.create_group("Work").unwrap().id().to_string();
        let friends_id = manager.create_group("Friends").unwrap().id().to_string();

        // Create field and associate with both labels
        let card = create_test_contact_card();
        let field_id = card.fields()[0].id();

        manager
            .get_group_mut(&work_id)
            .unwrap()
            .add_visible_field(field_id);
        manager
            .get_group_mut(&friends_id)
            .unwrap()
            .add_visible_field(field_id);

        // Verify field is visible to both labels
        assert!(
            manager
                .get_group(&work_id)
                .unwrap()
                .is_field_visible(field_id)
        );
        assert!(
            manager
                .get_group(&friends_id)
                .unwrap()
                .is_field_visible(field_id)
        );
    }

    // @scenario: visibility_control.feature:Add contact to group updates their visibility
    #[test]
    fn test_contact_label_assignment() {
        let mut manager = create_test_label_manager();

        // Create labels
        let label_id = manager.create_group("Family").unwrap().id().to_string();

        // Create contact ID
        let contact_id = "family-member-id";

        // Assign contact to label
        manager.add_contact_to_group(&label_id, contact_id).unwrap();

        // Verify assignment
        let contact_labels = manager.groups_for_contact(contact_id);
        assert_eq!(contact_labels.len(), 1);
        assert_eq!(contact_labels[0].id(), label_id);
    }

    // @scenario: visibility_control.feature:Add contact to group updates their visibility
    #[test]
    fn test_visibility_enforcement() {
        let mut manager = create_test_label_manager();

        // Create label and contact
        let label_id = manager.create_group("Restricted").unwrap().id().to_string();

        let field_id = "secret-field";
        let contact_id = "some-contact";

        // Associate field with label
        manager
            .get_group_mut(&label_id)
            .unwrap()
            .add_visible_field(field_id);

        // Test visibility: non-member cannot see field
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, None);

        // Add contact to label
        manager.add_contact_to_group(&label_id, contact_id).unwrap();

        // Test visibility: member can see field
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, Some(true));
    }

    // @scenario: visibility_control.feature:Hide a field from a specific contact
    #[test]
    fn test_per_contact_override() {
        let mut manager = create_test_label_manager();

        // Create label and field
        let label_id = manager.create_group("Group").unwrap().id().to_string();

        let field_id = "shared-field";
        let contact_id = "override-contact";

        // Associate field with label
        manager
            .get_group_mut(&label_id)
            .unwrap()
            .add_visible_field(field_id);

        // Grant override to specific contact
        manager.set_contact_override(contact_id, field_id, true);

        // Test visibility: override allows visibility even without label membership
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, Some(true));

        // Remove override
        manager.remove_contact_override(contact_id, field_id);

        // Test visibility: removed override requires label membership
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, None);
    }
}
