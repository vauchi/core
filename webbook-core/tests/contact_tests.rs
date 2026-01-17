//! Tests for contact
//! Extracted from mod.rs

use std::time::{SystemTime, UNIX_EPOCH};
use webbook_core::crypto::SymmetricKey;
use webbook_core::*;

fn create_test_contact() -> Contact {
    let public_key = [0u8; 32];
    let card = ContactCard::new("Test User");
    let shared_key = SymmetricKey::generate();

    Contact::from_exchange(public_key, card, shared_key)
}

#[test]
fn test_create_contact() {
    let contact = create_test_contact();

    assert!(!contact.id().is_empty());
    assert_eq!(contact.display_name(), "Test User");
    assert!(!contact.is_fingerprint_verified());
}

#[test]
fn test_fingerprint_verification() {
    let mut contact = create_test_contact();

    assert!(!contact.is_fingerprint_verified());
    contact.mark_fingerprint_verified();
    assert!(contact.is_fingerprint_verified());
}

#[test]
fn test_fingerprint_format() {
    let contact = create_test_contact();
    let fp = contact.fingerprint();

    // Should be formatted with spaces every 4 chars
    assert!(fp.contains(' '));
    // Should be uppercase
    assert_eq!(fp, fp.to_uppercase());
}

#[test]
fn test_visibility_rules() {
    let mut contact = create_test_contact();

    // Initially no specific rules
    assert!(contact
        .visibility_rules()
        .can_see("any_field", &contact.id()));

    // Set a field as private
    contact.visibility_rules_mut().set_nobody("private_field");
    assert!(!contact
        .visibility_rules()
        .can_see("private_field", &contact.id()));
}

// ============================================================
// Additional tests (added for coverage)
// ============================================================

#[test]
fn test_contact_from_sync_data() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Synced User");
    let shared_key = SymmetricKey::generate();
    let mut visibility_rules = VisibilityRules::new();
    visibility_rules.set_nobody("private_field");

    let contact = Contact::from_sync_data(
        public_key,
        card,
        shared_key,
        1234567890, // Specific timestamp
        true,       // Pre-verified
        visibility_rules,
    );

    assert_eq!(contact.display_name(), "Synced User");
    assert_eq!(contact.exchange_timestamp(), 1234567890);
    assert!(contact.is_fingerprint_verified());
    assert!(!contact
        .visibility_rules()
        .can_see("private_field", "anyone"));
}

#[test]
fn test_contact_update_card() {
    let mut contact = create_test_contact();
    assert_eq!(contact.display_name(), "Test User");

    // Update with new card
    let new_card = ContactCard::new("Updated User");
    contact.update_card(new_card);

    assert_eq!(contact.display_name(), "Updated User");
    assert_eq!(contact.card().display_name(), "Updated User");
}

#[test]
fn test_contact_set_display_name() {
    let mut contact = create_test_contact();

    contact.set_display_name("New Name").unwrap();
    assert_eq!(contact.display_name(), "New Name");
    assert_eq!(contact.card().display_name(), "New Name");
}

#[test]
fn test_contact_set_display_name_empty_error() {
    let mut contact = create_test_contact();

    let result = contact.set_display_name("");
    assert!(result.is_err());
}

#[test]
fn test_contact_accessors() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Alice");
    let shared_key = SymmetricKey::generate();

    let contact = Contact::from_exchange(public_key, card, shared_key.clone());

    // Test all accessors return correct values
    assert_eq!(contact.public_key(), &public_key);
    assert_eq!(contact.card().display_name(), "Alice");
    // shared_key returns reference, just verify it's accessible
    let _ = contact.shared_key();
    // exchange_timestamp should be recent (within last minute)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(contact.exchange_timestamp() <= now);
    assert!(contact.exchange_timestamp() > now - 60);
}

#[test]
fn test_contact_id_is_hex_encoded_public_key() {
    let public_key = [0xABu8; 32];
    let card = ContactCard::new("Test");
    let shared_key = SymmetricKey::generate();

    let contact = Contact::from_exchange(public_key, card, shared_key);

    // ID should be hex-encoded public key
    assert_eq!(contact.id(), hex::encode(public_key));
}

#[test]
fn test_fingerprint_readability() {
    let mut public_key = [0u8; 32];
    // Set known values for predictable fingerprint
    public_key[0] = 0xAB;
    public_key[1] = 0xCD;
    public_key[2] = 0xEF;
    public_key[3] = 0x01;

    let card = ContactCard::new("Test");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange(public_key, card, shared_key);

    let fp = contact.fingerprint();

    // Should start with known values grouped
    assert!(fp.starts_with("ABCD EF01"));
    // Should have proper spacing
    let parts: Vec<&str> = fp.split(' ').collect();
    assert!(parts.iter().all(|p| p.len() == 4));
}

// ============================================================
// Hidden Contacts Tests
// ============================================================

#[test]
fn test_contact_hidden_default_false() {
    let contact = create_test_contact();
    assert!(!contact.is_hidden());
    assert!(contact.is_visible_in_main_list());
}

#[test]
fn test_contact_hide_and_unhide() {
    let mut contact = create_test_contact();

    // Hide the contact
    contact.hide();
    assert!(contact.is_hidden());
    assert!(!contact.is_visible_in_main_list());

    // Unhide the contact
    contact.unhide();
    assert!(!contact.is_hidden());
    assert!(contact.is_visible_in_main_list());
}

#[test]
fn test_contact_set_hidden() {
    let mut contact = create_test_contact();

    contact.set_hidden(true);
    assert!(contact.is_hidden());

    contact.set_hidden(false);
    assert!(!contact.is_hidden());
}

// ============================================================
// Blocked Contacts Tests
// ============================================================

#[test]
fn test_contact_blocked_default_false() {
    let contact = create_test_contact();
    assert!(!contact.is_blocked());
    assert!(contact.should_process_updates());
    assert!(contact.should_send_updates());
}

#[test]
fn test_contact_block_and_unblock() {
    let mut contact = create_test_contact();

    // Block the contact
    contact.block();
    assert!(contact.is_blocked());
    assert!(!contact.should_process_updates());
    assert!(!contact.should_send_updates());

    // Unblock the contact
    contact.unblock();
    assert!(!contact.is_blocked());
    assert!(contact.should_process_updates());
    assert!(contact.should_send_updates());
}

#[test]
fn test_contact_set_blocked() {
    let mut contact = create_test_contact();

    contact.set_blocked(true);
    assert!(contact.is_blocked());

    contact.set_blocked(false);
    assert!(!contact.is_blocked());
}

#[test]
fn test_contact_hidden_and_blocked_independent() {
    let mut contact = create_test_contact();

    // Can be hidden but not blocked
    contact.hide();
    assert!(contact.is_hidden());
    assert!(!contact.is_blocked());
    assert!(contact.should_process_updates()); // Still processes updates

    // Can be blocked but not hidden
    contact.unhide();
    contact.block();
    assert!(!contact.is_hidden());
    assert!(contact.is_blocked());
    assert!(contact.is_visible_in_main_list()); // Still visible

    // Can be both hidden and blocked
    contact.hide();
    assert!(contact.is_hidden());
    assert!(contact.is_blocked());
    assert!(!contact.is_visible_in_main_list());
    assert!(!contact.should_process_updates());
}

#[test]
fn test_contact_from_sync_data_full() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Synced User");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = VisibilityRules::new();

    let contact = Contact::from_sync_data_full(
        public_key,
        card,
        shared_key,
        1234567890,
        true,
        visibility_rules,
        true, // hidden
        true, // blocked
    );

    assert!(contact.is_hidden());
    assert!(contact.is_blocked());
    assert!(contact.is_fingerprint_verified());
}
