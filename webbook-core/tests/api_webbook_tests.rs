//! Tests for api::webbook
//! Extracted from webbook.rs

use webbook_core::api::*;
use webbook_core::contact_card::FieldType;
use webbook_core::*;

fn create_test_webbook() -> WebBook<MockTransport> {
    WebBook::in_memory().unwrap()
}

#[test]
fn test_webbook_create_identity() {
    let mut wb = create_test_webbook();

    assert!(!wb.has_identity());

    wb.create_identity("Alice").unwrap();

    assert!(wb.has_identity());
    assert_eq!(wb.identity().unwrap().display_name(), "Alice");
}

#[test]
fn test_webbook_create_identity_twice_fails() {
    let mut wb = create_test_webbook();

    wb.create_identity("Alice").unwrap();

    let result = wb.create_identity("Bob");
    assert!(matches!(result, Err(WebBookError::AlreadyInitialized)));
}

#[test]
fn test_webbook_own_card() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), "Alice");
}

#[test]
fn test_webbook_update_own_card() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    let mut card = wb.own_card().unwrap().unwrap();
    let _ = card.add_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ));

    let changed = wb.update_own_card(&card).unwrap();
    assert!(changed.contains(&"email".to_string()));

    let loaded = wb.own_card().unwrap().unwrap();
    assert!(loaded.fields().iter().any(|f| f.label() == "email"));
}

#[test]
fn test_webbook_add_own_field() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    let field = ContactField::new(FieldType::Phone, "phone", "+1234567890");
    wb.add_own_field(field).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert!(card.fields().iter().any(|f| f.label() == "phone"));
}

#[test]
fn test_webbook_remove_own_field() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Add field
    let field = ContactField::new(FieldType::Phone, "phone", "+1234567890");
    wb.add_own_field(field).unwrap();

    // Remove field
    let removed = wb.remove_own_field("phone").unwrap();
    assert!(removed);

    let card = wb.own_card().unwrap().unwrap();
    assert!(!card.fields().iter().any(|f| f.label() == "phone"));
}

#[test]
fn test_webbook_contact_operations() {
    let wb = create_test_webbook();

    // Initially no contacts
    assert_eq!(wb.contact_count().unwrap(), 0);
    assert!(wb.list_contacts().unwrap().is_empty());

    // Add contact
    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Verify contact exists
    assert_eq!(wb.contact_count().unwrap(), 1);
    assert!(wb.get_contact(&contact_id).unwrap().is_some());

    // Search contacts
    let results = wb.search_contacts("bob").unwrap();
    assert_eq!(results.len(), 1);

    // Remove contact
    let removed = wb.remove_contact(&contact_id).unwrap();
    assert!(removed);
    assert_eq!(wb.contact_count().unwrap(), 0);
}

#[test]
fn test_webbook_verify_fingerprint() {
    let wb = create_test_webbook();

    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Initially not verified
    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(!loaded.is_fingerprint_verified());

    // Verify
    wb.verify_contact_fingerprint(&contact_id).unwrap();

    let loaded = wb.get_contact(&contact_id).unwrap().unwrap();
    assert!(loaded.is_fingerprint_verified());
}

#[test]
fn test_webbook_public_id() {
    let mut wb = create_test_webbook();

    // No identity yet
    let result = wb.public_id();
    assert!(matches!(result, Err(WebBookError::IdentityNotInitialized)));

    // Create identity
    wb.create_identity("Alice").unwrap();

    let public_id = wb.public_id().unwrap();
    assert!(!public_id.is_empty());
}

#[test]
fn test_webbook_builder() {
    let wb: WebBook<MockTransport> = WebBookBuilder::new()
        .storage_path("/tmp/test_webbook")
        .relay_url("wss://relay.example.com")
        .build()
        .unwrap();

    assert_eq!(wb.config().relay.server_url, "wss://relay.example.com");
}

#[test]
fn test_webbook_builder_with_identity() {
    let identity = Identity::create("Alice");
    let public_id = identity.public_id();

    let wb: WebBook<MockTransport> = WebBookBuilder::new()
        .storage_path("/tmp/test_webbook2")
        .identity(identity)
        .build()
        .unwrap();

    assert!(wb.has_identity());
    assert_eq!(wb.public_id().unwrap(), public_id);
}

#[test]
fn test_propagate_card_update_to_contacts() {
    use webbook_core::exchange::X3DHKeyPair;

    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Create a contact with ratchet
    let bob_key = [1u8; 32];
    let contact =
        Contact::from_exchange(bob_key, ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Initialize ratchet for Bob
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *their_dh.public_key())
        .unwrap();

    // Get old card, update it
    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ));

    // Propagate update
    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1);

    // Verify pending update was created
    let pending = wb.storage().get_pending_updates(&contact_id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].update_type, "card_delta");
}

#[test]
fn test_propagate_skips_contacts_without_ratchet() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Create a contact WITHOUT ratchet
    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    wb.add_contact(contact).unwrap();

    // Get old card, update it
    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "alice@company.com",
    ));

    // Propagate - should skip Bob (no ratchet)
    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 0);
}

#[test]
fn test_propagate_empty_delta_not_queued() {
    use webbook_core::exchange::X3DHKeyPair;

    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Create a contact with ratchet
    let contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    // Initialize ratchet
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *their_dh.public_key())
        .unwrap();

    // Propagate with identical cards (empty delta)
    let card = wb.own_card().unwrap().unwrap();
    let queued = wb.propagate_card_update(&card, &card).unwrap();
    assert_eq!(queued, 0);

    // Verify no pending updates
    let pending = wb.storage().get_pending_updates(&contact_id).unwrap();
    assert!(pending.is_empty());
}

#[test]
fn test_propagate_respects_visibility_rules() {
    use webbook_core::exchange::X3DHKeyPair;

    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Create the email field first to get its ID
    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com");
    let email_field_id = email_field.id().to_string();

    // Create a contact with ratchet
    let mut contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();

    // Set visibility: hide the email field (by its ID) from Bob
    contact.visibility_rules_mut().set_nobody(&email_field_id);
    wb.add_contact(contact).unwrap();

    // Initialize ratchet
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *their_dh.public_key())
        .unwrap();

    // Create old and new cards - add only email field
    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();

    // Add the email field (which is hidden from Bob)
    let _ = new_card.add_field(email_field);

    // Propagate - should skip Bob because he can't see email field
    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(
        queued, 0,
        "Update should not be queued when field is hidden from contact"
    );
}

#[test]
fn test_propagate_partial_visibility() {
    use webbook_core::exchange::X3DHKeyPair;

    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Create fields first to get their IDs
    let email_field = ContactField::new(FieldType::Email, "email", "alice@company.com");
    let email_field_id = email_field.id().to_string();
    let phone_field = ContactField::new(FieldType::Phone, "phone", "+1234567890");

    // Create a contact with ratchet
    let mut contact =
        Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let contact_id = contact.id().to_string();

    // Set visibility: hide only email field (by ID) from Bob
    contact.visibility_rules_mut().set_nobody(&email_field_id);
    wb.add_contact(contact).unwrap();

    // Initialize ratchet
    let shared_secret = SymmetricKey::generate();
    let their_dh = X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *their_dh.public_key())
        .unwrap();

    // Create cards with both email (hidden) and phone (visible) fields
    let old_card = wb.own_card().unwrap().unwrap();
    let mut new_card = old_card.clone();

    let _ = new_card.add_field(email_field);
    let _ = new_card.add_field(phone_field);

    // Propagate - should queue update with only phone field
    let queued = wb.propagate_card_update(&old_card, &new_card).unwrap();
    assert_eq!(queued, 1, "Update should be queued for visible field");

    // Verify the pending update was created
    let pending = wb.storage().get_pending_updates(&contact_id).unwrap();
    assert_eq!(pending.len(), 1);
}

#[test]
fn test_process_incoming_card_update() {
    use webbook_core::crypto::ratchet::DoubleRatchetState;
    use webbook_core::exchange::X3DHKeyPair;
    use webbook_core::sync::delta::CardDelta;
    use webbook_core::Identity;

    // Create Alice's WebBook
    let mut alice_wb = create_test_webbook();
    alice_wb.create_identity("Alice").unwrap();

    // Create Bob's identity and keypair for ratchet
    let bob_identity = Identity::create("Bob");
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    // Add Bob as a contact on Alice's side
    let contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
    );
    let bob_id = contact.id().to_string();
    alice_wb.add_contact(contact).unwrap();

    // Initialize ratchet as responder (Alice will receive from Bob)
    alice_wb
        .create_ratchet_as_responder(
            &bob_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(bob_dh.secret_bytes()),
        )
        .unwrap();

    // Bob creates and encrypts an update
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    // Create a delta (Bob adds an email field)
    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "bob@company.com",
    ));

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&bob_identity);

    // Encrypt the delta
    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&delta_bytes).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Alice processes the incoming update
    let changed = alice_wb.process_card_update(&bob_id, &encrypted).unwrap();

    // Verify the changes were applied
    assert!(!changed.is_empty());
    assert!(changed.iter().any(|f| f == "work"));

    // Verify Bob's card was updated
    let bob_contact = alice_wb.get_contact(&bob_id).unwrap().unwrap();
    let bob_card = bob_contact.card();
    assert!(bob_card.fields().iter().any(|f| f.label() == "work"));
}

#[test]
fn test_update_display_name() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Verify initial name
    assert_eq!(wb.identity().unwrap().display_name(), "Alice");
    assert_eq!(wb.own_card().unwrap().unwrap().display_name(), "Alice");

    // Update display name
    wb.update_display_name("Alice Smith").unwrap();

    // Verify both identity and card are updated
    assert_eq!(wb.identity().unwrap().display_name(), "Alice Smith");
    assert_eq!(
        wb.own_card().unwrap().unwrap().display_name(),
        "Alice Smith"
    );
}

#[test]
fn test_update_display_name_empty_fails() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Empty name should fail
    let result = wb.update_display_name("");
    assert!(result.is_err());

    // Whitespace-only should fail
    let result = wb.update_display_name("   ");
    assert!(result.is_err());
}

#[test]
fn test_update_display_name_too_long_fails() {
    let mut wb = create_test_webbook();
    wb.create_identity("Alice").unwrap();

    // Name over 100 chars should fail
    let long_name = "a".repeat(101);
    let result = wb.update_display_name(&long_name);
    assert!(result.is_err());
}

#[test]
fn test_update_display_name_no_identity_fails() {
    let mut wb = create_test_webbook();

    // No identity yet
    let result = wb.update_display_name("Alice");
    assert!(matches!(result, Err(WebBookError::IdentityNotInitialized)));
}

#[test]
fn test_process_update_rejects_invalid_signature() {
    use webbook_core::crypto::ratchet::DoubleRatchetState;
    use webbook_core::exchange::X3DHKeyPair;
    use webbook_core::sync::delta::CardDelta;
    use webbook_core::Identity;

    let mut alice_wb = create_test_webbook();
    alice_wb.create_identity("Alice").unwrap();

    // Create Bob's identity
    let bob_identity = Identity::create("Bob");
    let bob_dh = X3DHKeyPair::generate();
    let shared_secret = SymmetricKey::generate();

    // Add Bob as a contact
    let contact = Contact::from_exchange(
        *bob_identity.signing_public_key(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
    );
    let bob_id = contact.id().to_string();
    alice_wb.add_contact(contact).unwrap();

    // Initialize ratchet
    alice_wb
        .create_ratchet_as_responder(
            &bob_id,
            &shared_secret,
            X3DHKeyPair::from_bytes(bob_dh.secret_bytes()),
        )
        .unwrap();

    // Create update signed by WRONG identity (not Bob)
    let wrong_identity = Identity::create("Eve");
    let mut bob_ratchet =
        DoubleRatchetState::initialize_initiator(&shared_secret, *bob_dh.public_key());

    let old_card = ContactCard::new("Bob");
    let mut new_card = ContactCard::new("Bob");
    let _ = new_card.add_field(ContactField::new(
        FieldType::Email,
        "work",
        "bob@company.com",
    ));

    let mut delta = CardDelta::compute(&old_card, &new_card);
    delta.sign(&wrong_identity); // WRONG signature!

    let delta_bytes = serde_json::to_vec(&delta).unwrap();
    let ratchet_msg = bob_ratchet.encrypt(&delta_bytes).unwrap();
    let encrypted = serde_json::to_vec(&ratchet_msg).unwrap();

    // Should fail signature verification
    let result = alice_wb.process_card_update(&bob_id, &encrypted);
    assert!(matches!(result, Err(WebBookError::SignatureInvalid)));
}
