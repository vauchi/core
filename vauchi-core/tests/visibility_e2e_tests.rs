use vauchi_core::{
    api::Vauchi,
    contact::LabelManager,
};

#[cfg(test)]
mod visibility_e2e_tests {
    use super::*;

    fn create_test_vauchi() -> Vauchi {
        Vauchi::in_memory().unwrap()
    }

    fn setup_vauchi_with_labels() -> (Vauchi, String, String) {
        let mut vauchi = create_test_vauchi();
        vauchi.create_identity("Test User").unwrap();

        let mut label_manager = LabelManager::new();
        let work_id = label_manager.create_label("Work").unwrap().id().to_string();
        let personal_id = label_manager
            .create_label("Personal")
            .unwrap()
            .id()
            .to_string();

        // In a real scenario, we'd save the label manager to vauchi's storage
        // For E2E testing of the logic, we can use the manager directly

        (vauchi, work_id, personal_id)
    }

    #[test]
    fn test_visibility_logic_e2e() {
        let (_vauchi, _work_id, _personal_id) = setup_vauchi_with_labels();
        let mut label_manager = LabelManager::new();
        label_manager.create_label("Work").unwrap();
        label_manager.create_label("Personal").unwrap();

        let work_label_id = label_manager
            .get_label_by_name("Work")
            .unwrap()
            .id()
            .to_string();
        let personal_label_id = label_manager
            .get_label_by_name("Personal")
            .unwrap()
            .id()
            .to_string();

        let contact_id = "bob-id";
        let email_field_id = "email-id";
        let phone_field_id = "phone-id";

        // Assign fields to labels
        label_manager
            .get_label_mut(&work_label_id)
            .unwrap()
            .add_visible_field(email_field_id);
        label_manager
            .get_label_mut(&personal_label_id)
            .unwrap()
            .add_visible_field(phone_field_id);

        // Assign Bob to Work label
        label_manager
            .add_contact_to_label(&work_label_id, contact_id)
            .unwrap();

        // Bob should see email but not phone
        assert_eq!(
            label_manager.can_see_via_labels(contact_id, email_field_id),
            Some(true)
        );
        assert_eq!(
            label_manager.can_see_via_labels(contact_id, phone_field_id),
            None
        );

        // Assign Bob to Personal label too
        label_manager
            .add_contact_to_label(&personal_label_id, contact_id)
            .unwrap();

        // Now Bob should see both
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
