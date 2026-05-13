// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact limit enforcement (SP-12a).
//!
//! @scenario: contacts_management :: Maximum contacts reached
//! @scenario: contacts_management :: Exceed maximum contacts

use crate::common;

use vauchi_core::{Contact, ContactCard, SymmetricKey, VauchiError};

fn make_contact(name: &str) -> Contact {
    let pk = *vauchi_core::SigningKeyPair::generate()
        .public_key()
        .as_bytes();
    let card = ContactCard::new(name);
    Contact::from_exchange(pk, card, SymmetricKey::generate(), 0)
}

// @scenario: contacts_management :: Maximum contacts reached
// @internal
#[test]
fn test_add_contact_at_limit_minus_one_succeeds() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");

    // Set a low limit for testing (avoid creating 10,000 contacts in a unit test)
    wb.storage().set_contact_limit(3).unwrap();

    // Add contacts up to limit - 1
    wb.add_contact(make_contact("Bob")).unwrap();
    wb.add_contact(make_contact("Carol")).unwrap();

    // Adding the 3rd contact (at the limit) should succeed
    let result = wb.add_contact(make_contact("Dave"));
    assert!(result.is_ok(), "Adding contact at limit should succeed");
    assert_eq!(wb.contact_count().unwrap(), 3);
}

// @scenario: contacts_management :: Maximum contacts reached
// @internal
#[test]
fn test_add_contact_reaches_exact_limit() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");

    // Set limit to 2
    wb.storage().set_contact_limit(2).unwrap();

    wb.add_contact(make_contact("Bob")).unwrap();
    let result = wb.add_contact(make_contact("Carol"));
    assert!(
        result.is_ok(),
        "Adding contact at exact limit should succeed"
    );
    assert_eq!(wb.contact_count().unwrap(), 2);
}

// @scenario: contacts_management :: Exceed maximum contacts
// @internal
#[test]
fn test_add_contact_exceeds_limit_returns_error() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");

    // Set limit to 2
    wb.storage().set_contact_limit(2).unwrap();

    wb.add_contact(make_contact("Bob")).unwrap();
    wb.add_contact(make_contact("Carol")).unwrap();

    // The 3rd contact should be rejected
    let result = wb.add_contact(make_contact("Dave"));
    assert!(
        matches!(&result, Err(VauchiError::ContactLimitReached(2))),
        "Expected ContactLimitReached(2), got {:?}",
        result
    );
}

// @scenario: contacts_management :: Exceed maximum contacts
// @internal
#[test]
fn test_exceed_limit_error_message_includes_limit() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    wb.storage().set_contact_limit(1).unwrap();

    wb.add_contact(make_contact("Bob")).unwrap();

    let err = wb.add_contact(make_contact("Carol")).unwrap_err();
    let msg = format!("{}", err);
    assert!(
        msg.contains("1"),
        "Error message should include the limit, got: {}",
        msg
    );
    assert!(
        msg.contains("contact limit reached"),
        "Error message should mention limit reached, got: {}",
        msg
    );
}

// @scenario: contacts_management :: Exceed maximum contacts
// @internal
#[test]
fn test_exceed_limit_contact_not_persisted() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    wb.storage().set_contact_limit(1).unwrap();

    wb.add_contact(make_contact("Bob")).unwrap();
    let _ = wb.add_contact(make_contact("Carol")); // Should fail

    // Only 1 contact should be stored
    assert_eq!(wb.contact_count().unwrap(), 1);
    let contacts = wb.list_contacts().unwrap();
    assert_eq!(contacts.len(), 1);
    assert_eq!(contacts[0].display_name(), "Bob");
}

// @scenario: contacts_management :: Maximum contacts reached
// @internal
#[test]
fn test_add_contact_after_removal_succeeds() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    wb.storage().set_contact_limit(2).unwrap();

    let bob = make_contact("Bob");
    let bob_id = bob.id().to_string();
    wb.add_contact(bob).unwrap();
    wb.add_contact(make_contact("Carol")).unwrap();

    // At limit, remove one
    wb.remove_contact(&bob_id).unwrap();

    // Should be able to add again
    let result = wb.add_contact(make_contact("Dave"));
    assert!(result.is_ok(), "Should succeed after removing a contact");
    assert_eq!(wb.contact_count().unwrap(), 2);
}

// @scenario: contacts_management :: Maximum contacts reached
// @internal
#[test]
fn test_default_limit_is_10000() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    let limit = wb.storage().get_contact_limit().unwrap();
    assert_eq!(limit, 10_000, "Default contact limit should be 10,000");
}

// Negative path: limit of 0 means no contacts allowed
// @internal
#[test]
fn test_zero_limit_rejects_all_contacts() {
    let wb = common::helpers::create_vauchi_with_identity("Alice");
    wb.storage().set_contact_limit(0).unwrap();

    let result = wb.add_contact(make_contact("Bob"));
    assert!(
        matches!(result, Err(VauchiError::ContactLimitReached(0))),
        "Zero limit should reject all contacts, got {:?}",
        result
    );
}
