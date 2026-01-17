//! Visibility Control E2E Tests
//!
//! Feature: visibility_control.feature

use webbook_core::{
    network::MockTransport, Contact, ContactCard, ContactField, FieldType, SymmetricKey, WebBook,
};

/// Tests the visibility control workflow.
///
/// Feature: visibility_control.feature
/// Scenarios: Hide field from specific contact, Visibility propagation
#[test]
fn test_visibility_control_happy_path() {
    // Step 1: Create Alice's WebBook with contacts
    let mut alice_wb: WebBook<MockTransport> = WebBook::in_memory().unwrap();
    alice_wb.create_identity("Alice").unwrap();

    // Add fields to Alice's card
    alice_wb
        .add_own_field(ContactField::new(FieldType::Phone, "work", "+15551111111"))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Phone,
            "personal",
            "+15552222222",
        ))
        .unwrap();
    alice_wb
        .add_own_field(ContactField::new(
            FieldType::Email,
            "work",
            "alice@company.com",
        ))
        .unwrap();

    // Step 2: Create contacts Bob and Carol
    let bob_public_key = [1u8; 32];
    let carol_public_key = [2u8; 32];

    let bob_contact = Contact::from_exchange(
        bob_public_key,
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
    );
    let bob_id = bob_contact.id().to_string();

    let carol_contact = Contact::from_exchange(
        carol_public_key,
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
    );
    let carol_id = carol_contact.id().to_string();

    alice_wb.add_contact(bob_contact).unwrap();
    alice_wb.add_contact(carol_contact).unwrap();

    // Step 3: Set visibility rules - hide "personal" from Bob
    let mut bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    bob_contact.visibility_rules_mut().set_nobody("personal");
    alice_wb.storage().save_contact(&bob_contact).unwrap();

    // Step 4: Get Alice's card filtered for each contact
    let alice_card = alice_wb.own_card().unwrap().unwrap();

    let bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    let bob_rules = bob_contact.visibility_rules();

    let carol_contact = alice_wb.get_contact(&carol_id).unwrap().unwrap();
    let carol_rules = carol_contact.visibility_rules();

    // Step 5: Verify visibility filtering
    assert!(
        bob_rules.can_see("work", &bob_id),
        "Bob should see work phone"
    );
    assert!(
        !bob_rules.can_see("personal", &bob_id),
        "Bob should NOT see personal phone"
    );

    assert!(
        carol_rules.can_see("work", &carol_id),
        "Carol should see work phone"
    );
    assert!(
        carol_rules.can_see("personal", &carol_id),
        "Carol should see personal phone"
    );

    // Step 6: Filter card based on visibility rules
    let bob_visible_fields: Vec<_> = alice_card
        .fields()
        .iter()
        .filter(|f| bob_rules.can_see(f.label(), &bob_id))
        .collect();

    let carol_visible_fields: Vec<_> = alice_card
        .fields()
        .iter()
        .filter(|f| carol_rules.can_see(f.label(), &carol_id))
        .collect();

    assert_eq!(bob_visible_fields.len(), 2);
    assert!(bob_visible_fields.iter().all(|f| f.label() != "personal"));
    assert_eq!(carol_visible_fields.len(), 3);

    // Step 7: Test granting visibility back
    let mut bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    bob_contact.visibility_rules_mut().set_everyone("personal");
    alice_wb.storage().save_contact(&bob_contact).unwrap();

    let bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    let bob_rules = bob_contact.visibility_rules();
    assert!(
        bob_rules.can_see("personal", &bob_id),
        "Bob should now see personal phone"
    );
}
