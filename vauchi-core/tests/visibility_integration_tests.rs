// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{
    contact::LabelManager,
    contact_card::{ContactCard, ContactField, FieldType},
};

#[cfg(test)]
mod visibility_integration_tests {
    use super::*;

    fn create_test_label_manager() -> LabelManager {
        LabelManager::new()
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

    #[test]
    fn test_label_creation() {
        let mut manager = create_test_label_manager();
        let label = manager.create_label("Work Contacts").unwrap();
        assert_eq!(label.name(), "Work Contacts");
        assert_eq!(manager.label_count(), 1);
    }

    #[test]
    fn test_field_label_association() {
        let mut manager = create_test_label_manager();

        // Create a label
        let label_id = manager
            .create_label("Work Contacts")
            .unwrap()
            .id()
            .to_string();

        // Create card and get field ID
        let card = create_test_contact_card();
        let field_id = card.fields()[0].id();

        // Associate field with label
        let label = manager.get_label_mut(&label_id).unwrap();
        label.add_visible_field(field_id);

        // Verify association
        let label = manager.get_label(&label_id).unwrap();
        assert!(label.is_field_visible(field_id));
    }

    #[test]
    fn test_multiple_label_field_visibility() {
        let mut manager = create_test_label_manager();

        // Create multiple labels
        let work_id = manager.create_label("Work").unwrap().id().to_string();
        let friends_id = manager.create_label("Friends").unwrap().id().to_string();

        // Create field and associate with both labels
        let card = create_test_contact_card();
        let field_id = card.fields()[0].id();

        manager
            .get_label_mut(&work_id)
            .unwrap()
            .add_visible_field(field_id);
        manager
            .get_label_mut(&friends_id)
            .unwrap()
            .add_visible_field(field_id);

        // Verify field is visible to both labels
        assert!(manager
            .get_label(&work_id)
            .unwrap()
            .is_field_visible(field_id));
        assert!(manager
            .get_label(&friends_id)
            .unwrap()
            .is_field_visible(field_id));
    }

    #[test]
    fn test_contact_label_assignment() {
        let mut manager = create_test_label_manager();

        // Create labels
        let label_id = manager.create_label("Family").unwrap().id().to_string();

        // Create contact ID
        let contact_id = "family-member-id";

        // Assign contact to label
        manager.add_contact_to_label(&label_id, contact_id).unwrap();

        // Verify assignment
        let contact_labels = manager.labels_for_contact(contact_id);
        assert_eq!(contact_labels.len(), 1);
        assert_eq!(contact_labels[0].id(), label_id);
    }

    #[test]
    fn test_visibility_enforcement() {
        let mut manager = create_test_label_manager();

        // Create label and contact
        let label_id = manager.create_label("Restricted").unwrap().id().to_string();

        let field_id = "secret-field";
        let contact_id = "some-contact";

        // Associate field with label
        manager
            .get_label_mut(&label_id)
            .unwrap()
            .add_visible_field(field_id);

        // Test visibility: non-member cannot see field
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, None);

        // Add contact to label
        manager.add_contact_to_label(&label_id, contact_id).unwrap();

        // Test visibility: member can see field
        let can_see = manager.can_see_via_labels(contact_id, field_id);
        assert_eq!(can_see, Some(true));
    }

    #[test]
    fn test_per_contact_override() {
        let mut manager = create_test_label_manager();

        // Create label and field
        let label_id = manager.create_label("Group").unwrap().id().to_string();

        let field_id = "shared-field";
        let contact_id = "override-contact";

        // Associate field with label
        manager
            .get_label_mut(&label_id)
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
