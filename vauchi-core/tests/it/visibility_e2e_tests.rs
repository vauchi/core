// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::{api::Vauchi, contact::GroupManager};

#[cfg(test)]
mod visibility_e2e_tests {
    use super::*;

    fn create_test_vauchi() -> Vauchi {
        Vauchi::in_memory().unwrap()
    }

    fn setup_vauchi_with_labels() -> (Vauchi, String, String) {
        let mut vauchi = create_test_vauchi();
        vauchi.create_identity("Test User").unwrap();

        let mut label_manager = GroupManager::new();
        let work_id = label_manager
            .create_group("Work", 0)
            .unwrap()
            .id()
            .to_string();
        let personal_id = label_manager
            .create_group("Personal", 0)
            .unwrap()
            .id()
            .to_string();

        // In a real scenario, we'd save the label manager to vauchi's storage
        // For E2E testing of the logic, we can use the manager directly

        (vauchi, work_id, personal_id)
    }

    // @scenario: visibility_control :: Add contact to group updates their visibility
    // @scenario: visibility_control :: Apply visibility group to a field
    #[test]
    fn test_visibility_logic_e2e() {
        let (_vauchi, _work_id, _personal_id) = setup_vauchi_with_labels();
        let mut label_manager = GroupManager::new();
        label_manager.create_group("Work", 0).unwrap();
        label_manager.create_group("Personal", 0).unwrap();

        let work_label_id = label_manager
            .get_group_by_name("Work")
            .unwrap()
            .id()
            .to_string();
        let personal_label_id = label_manager
            .get_group_by_name("Personal")
            .unwrap()
            .id()
            .to_string();

        let contact_id = "bob-id";
        let email_field_id = "email-id";
        let phone_field_id = "phone-id";

        label_manager
            .get_group_mut(&work_label_id)
            .unwrap()
            .add_visible_field(email_field_id, 0);
        label_manager
            .get_group_mut(&personal_label_id)
            .unwrap()
            .add_visible_field(phone_field_id, 0);

        label_manager
            .add_contact_to_group(&work_label_id, contact_id, 0)
            .unwrap();

        assert_eq!(
            label_manager.can_see_via_labels(contact_id, email_field_id),
            Some(true)
        );
        assert_eq!(
            label_manager.can_see_via_labels(contact_id, phone_field_id),
            None
        );

        label_manager
            .add_contact_to_group(&personal_label_id, contact_id, 0)
            .unwrap();

        assert_eq!(
            label_manager.can_see_via_labels(contact_id, email_field_id),
            Some(true)
        );
        assert_eq!(
            label_manager.can_see_via_labels(contact_id, phone_field_id),
            Some(true)
        );
    }
}
