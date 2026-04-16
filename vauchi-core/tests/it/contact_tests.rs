// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact
//! Extracted from mod.rs

use std::time::{SystemTime, UNIX_EPOCH};
use vauchi_core::contact::TrustLevel;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::*;

fn create_test_contact() -> Contact {
    let public_key = [0u8; 32];
    let card = ContactCard::new("Test User");
    let shared_key = SymmetricKey::generate();

    Contact::from_exchange(public_key, card, shared_key)
}

// @scenario: contacts_management :: View contact details
// @internal
#[test]
fn test_create_contact() {
    let contact = create_test_contact();

    assert!(!contact.id().is_empty());
    assert_eq!(contact.display_name(), "Test User");
    assert!(!contact.is_fingerprint_verified());
}

// @scenario: security :: Fingerprint verification
// @internal
#[test]
fn test_fingerprint_verification() {
    let mut contact = create_test_contact();

    assert!(!contact.is_fingerprint_verified());
    contact.mark_fingerprint_verified().unwrap();
    assert!(contact.is_fingerprint_verified());
}

// @scenario: security :: Fingerprint verification
// @internal
#[test]
fn test_fingerprint_format() {
    let contact = create_test_contact();
    let fp = contact.fingerprint();

    // Should be formatted with spaces every 4 chars
    assert!(fp.contains(' '));
    // Should be uppercase
    assert_eq!(fp, fp.to_uppercase());
}

// @scenario: visibility_control :: Default visibility is public
// @scenario: visibility_control :: Set field visibility to private
// @internal
#[test]
fn test_visibility_rules() {
    let mut contact = create_test_contact();

    // Initially no specific rules
    assert!(
        contact
            .visibility_rules()
            .unwrap()
            .can_see("any_field", contact.id())
    );

    // Set a field as private
    contact
        .visibility_rules_mut()
        .unwrap()
        .set_nobody("private_field");
    assert!(
        !contact
            .visibility_rules()
            .unwrap()
            .can_see("private_field", contact.id())
    );
}

// ============================================================
// Additional tests (added for coverage)
// ============================================================

// @scenario: sync_updates :: Receive contact update from relay
// @internal
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
    assert_eq!(contact.exchange_timestamp().unwrap(), 1234567890);
    assert!(contact.is_fingerprint_verified());
    assert!(
        !contact
            .visibility_rules()
            .unwrap()
            .can_see("private_field", "anyone")
    );
}

// @scenario: sync_updates :: Receive contact update from relay
// @internal
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

// @scenario: contact_card_management :: Update display name
// @internal
#[test]
fn test_contact_set_display_name() {
    let mut contact = create_test_contact();

    contact.set_display_name("New Name").unwrap();
    assert_eq!(contact.display_name(), "New Name");
    assert_eq!(contact.card().display_name(), "New Name");
}

// @scenario: contact_card_management :: Display name must not be empty
// @internal
#[test]
fn test_contact_set_display_name_empty_error() {
    let mut contact = create_test_contact();

    let result = contact.set_display_name("");
    result.expect_err("expected error");
}

// @scenario: contacts_management :: View contact details
// @internal
#[test]
fn test_contact_accessors() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Alice");
    let shared_key = SymmetricKey::generate();

    let contact = Contact::from_exchange(public_key, card, shared_key.clone());

    // Test all accessors return correct values
    assert_eq!(contact.public_key().unwrap(), &public_key);
    assert_eq!(contact.card().display_name(), "Alice");
    // shared_key returns reference, just verify it's accessible
    let _ = contact.shared_key();
    // exchange_timestamp should be recent (within last minute)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(contact.exchange_timestamp().unwrap() <= now);
    assert!(contact.exchange_timestamp().unwrap() > now - 60);
}

// @scenario: contacts_management :: View contact details
// @internal
#[test]
fn test_contact_id_is_hex_encoded_public_key() {
    let public_key = [0xABu8; 32];
    let card = ContactCard::new("Test");
    let shared_key = SymmetricKey::generate();

    let contact = Contact::from_exchange(public_key, card, shared_key);

    // ID should be hex-encoded public key
    assert_eq!(contact.id(), hex::encode(public_key));
}

// @scenario: security :: Fingerprint verification
// @internal
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

// @scenario: contacts_management :: Hide contact from main list
// @internal
#[test]
fn test_contact_hidden_default_false() {
    let contact = create_test_contact();
    assert!(!contact.is_hidden());
    assert!(contact.is_visible_in_main_list());
}

// @scenario: contacts_management :: Hide contact from main list
// @internal
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

// @scenario: contacts_management :: Hide contact from main list
// @internal
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

// @scenario: contacts_management :: Block a contact
// @internal
#[test]
fn test_contact_blocked_default_false() {
    let contact = create_test_contact();
    assert!(!contact.is_blocked());
    assert!(contact.should_process_updates());
    assert!(contact.should_send_updates());
}

// @scenario: contacts_management :: Block a contact
// @scenario: contacts_management :: Unblock a contact
// @internal
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

// @scenario: contacts_management :: Block a contact
// @internal
#[test]
fn test_contact_set_blocked() {
    let mut contact = create_test_contact();

    contact.set_blocked(true);
    assert!(contact.is_blocked());

    contact.set_blocked(false);
    assert!(!contact.is_blocked());
}

// @scenario: contacts_management :: Block a contact
// @internal
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

// @scenario: sync_updates :: Receive contact update from relay
// @internal
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
        true,  // hidden
        true,  // blocked
        false, // recovery_trusted
    );

    assert!(contact.is_hidden());
    assert!(contact.is_blocked());
    assert!(contact.is_fingerprint_verified());
    assert!(!contact.is_recovery_trusted());
}

// ========================================
// Recovery Trust Tests
// ========================================

// @scenario: identity_management :: Social recovery setup
// @internal
#[test]
fn test_contact_default_not_recovery_trusted() {
    let contact = create_test_contact();
    assert!(!contact.is_recovery_trusted());
}

// @scenario: identity_management :: Social recovery setup
// @internal
#[test]
fn test_contact_trust_for_recovery() {
    let mut contact = create_test_contact();
    // Must be fingerprint-verified (Verified trust) to grant recovery
    contact.mark_fingerprint_verified().unwrap();
    assert!(!contact.is_recovery_trusted());

    contact.trust_for_recovery().unwrap();
    assert!(contact.is_recovery_trusted());
}

// @scenario: identity_management :: Social recovery setup
// @internal
#[test]
fn test_contact_trust_for_recovery_rejects_standard_trust() {
    let mut contact = create_test_contact();
    // Standard trust (no verification) — must be rejected
    let result = contact.trust_for_recovery();
    assert!(
        result.is_err(),
        "Standard-trust contact must not be recovery-trusted"
    );
}

// @scenario: identity_management :: Social recovery setup
// @internal
#[test]
fn test_contact_trust_for_recovery_rejects_blocked() {
    let mut contact = create_test_contact();
    contact.mark_fingerprint_verified().unwrap();
    contact.set_blocked(true);

    let result = contact.trust_for_recovery();
    assert!(
        result.is_err(),
        "Blocked contacts must not be recovery-trusted"
    );
    assert!(
        matches!(result, Err(ContactError::ContactIsBlocked)),
        "Expected ContactIsBlocked error, got {:?}",
        result
    );
}

// @scenario: contact_recovery :: Recovered contact trust lifecycle
//
// Principle 2: a recovered identity drops to Cautious. Recovery
// trust is blocked until the user re-verifies the fingerprint
// in person, which clears the recovered flag and restores
// Verified trust.
// @internal
#[test]
fn test_recovered_contact_trust_lifecycle() {
    let mut contact = create_test_contact();
    contact.mark_fingerprint_verified().unwrap();
    assert_eq!(contact.trust_level(), TrustLevel::Verified);
    contact.trust_for_recovery().unwrap();

    // Simulate recovery: trust drops to Cautious
    contact.untrust_for_recovery().unwrap();
    let new_key = SymmetricKey::generate();
    contact.accept_recovery([99u8; 32], new_key).unwrap();
    assert_eq!(contact.trust_level(), TrustLevel::Cautious);

    // Cautious blocks recovery trust
    let result = contact.trust_for_recovery();
    assert!(
        result.is_err(),
        "Cautious contact must not be recovery-trusted"
    );

    // Re-verify fingerprint in person: clears recovered flag
    contact.mark_fingerprint_verified().unwrap();
    assert_eq!(contact.trust_level(), TrustLevel::Verified);

    // Now recovery trust works again
    contact.trust_for_recovery().unwrap();
    assert!(contact.is_recovery_trusted());
}

// @scenario: identity_management :: Social recovery setup
// @scenario: contact_recovery :: Remove recovery trust from contact
// @internal
#[test]
fn test_contact_untrust_for_recovery() {
    let mut contact = create_test_contact();
    contact.mark_fingerprint_verified().unwrap();
    contact.trust_for_recovery().unwrap();
    assert!(contact.is_recovery_trusted());

    contact.untrust_for_recovery().unwrap();
    assert!(!contact.is_recovery_trusted());
}

// @scenario: identity_management :: Social recovery setup
// @internal
#[test]
fn test_contact_set_recovery_trusted() {
    let mut contact = create_test_contact();

    contact.set_recovery_trusted(true).unwrap();
    assert!(contact.is_recovery_trusted());

    contact.set_recovery_trusted(false).unwrap();
    assert!(!contact.is_recovery_trusted());
}

// @scenario: identity_management :: Social recovery setup
// @scenario: sync_updates :: Receive contact update from relay
// @scenario: contact_recovery :: Trust state syncs across linked devices
// @internal
#[test]
fn test_contact_from_sync_data_full_with_recovery_trusted() {
    let public_key = [0x42u8; 32];
    let card = ContactCard::new("Trusted User");
    let shared_key = SymmetricKey::generate();
    let visibility_rules = VisibilityRules::new();

    let contact = Contact::from_sync_data_full(
        public_key,
        card,
        shared_key,
        1234567890,
        false,
        visibility_rules,
        false, // hidden
        false, // blocked
        true,  // recovery_trusted
    );

    assert!(contact.is_recovery_trusted());
    assert!(!contact.is_hidden());
    assert!(!contact.is_blocked());
}

// @scenario: identity_management :: Social recovery setup
// @scenario: contact_recovery :: Removing trust does not affect other contact properties
// @internal
#[test]
fn test_recovery_trust_independent_of_blocked_hidden() {
    let mut contact = create_test_contact();
    contact.mark_fingerprint_verified().unwrap();

    // Trust + block
    contact.trust_for_recovery().unwrap();
    contact.block();
    assert!(contact.is_recovery_trusted());
    assert!(contact.is_blocked());

    // Trust + hide
    contact.unblock();
    contact.hide();
    assert!(contact.is_recovery_trusted());
    assert!(contact.is_hidden());

    // Untrust doesn't affect other flags
    contact.untrust_for_recovery().unwrap();
    assert!(!contact.is_recovery_trusted());
    assert!(contact.is_hidden());
}
