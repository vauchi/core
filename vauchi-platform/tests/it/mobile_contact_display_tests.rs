// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Unit tests for MobileContact::with_display_context.
//!
//! Verifies that the constructor correctly populates `resolved_display_name`,
//! `nickname`, and `has_custom_avatar` without requiring a full platform setup.
//!
//! @scenario: contacts_management.feature - Display name resolution in platform bindings

use vauchi_core::{Contact, ContactCard, SymmetricKey};
use vauchi_platform::MobileContact;

/// Build a minimal exchanged contact with the given display name.
fn make_contact(name: &str) -> Contact {
    let mut pk = [0u8; 32];
    pk[0] = 0x42;
    let card = ContactCard::new(name);
    Contact::from_exchange(pk, card, SymmetricKey::generate())
}

// @scenario: contacts_management.feature :: Display name resolution in platform bindings
#[test]
fn with_display_context_sets_resolved_display_name() {
    let contact = make_contact("Bob");
    let mc = MobileContact::with_display_context(
        &contact,
        Some("Bobby".to_string()),
        "Bobby".to_string(),
        false,
    );
    assert_eq!(
        mc.resolved_display_name, "Bobby",
        "resolved_display_name must match the provided resolved string"
    );
}

// @scenario: contacts_management.feature :: Display name resolution in platform bindings
#[test]
fn with_display_context_sets_nickname() {
    let contact = make_contact("Bob");
    let mc = MobileContact::with_display_context(
        &contact,
        Some("Bobby".to_string()),
        "Bobby".to_string(),
        false,
    );
    assert_eq!(
        mc.nickname,
        Some("Bobby".to_string()),
        "nickname must match the provided Option<String>"
    );
}

// @internal
#[test]
fn with_display_context_none_nickname() {
    let contact = make_contact("Bob");
    let mc = MobileContact::with_display_context(&contact, None, "Bob".to_string(), false);
    assert!(
        mc.nickname.is_none(),
        "nickname must be None when not provided"
    );
    assert_eq!(
        mc.resolved_display_name, "Bob",
        "resolved_display_name must still be populated when nickname is None"
    );
}

// @internal
#[test]
fn with_display_context_has_custom_avatar_true() {
    let contact = make_contact("Bob");
    let mc = MobileContact::with_display_context(
        &contact,
        None,
        "Bob".to_string(),
        true, // has_custom_avatar
    );
    assert!(
        mc.has_custom_avatar,
        "has_custom_avatar must be true when set"
    );
}

// @internal
#[test]
fn with_display_context_has_custom_avatar_false() {
    let contact = make_contact("Bob");
    let mc = MobileContact::with_display_context(&contact, None, "Bob".to_string(), false);
    assert!(
        !mc.has_custom_avatar,
        "has_custom_avatar must be false when not set"
    );
}

// @internal
#[test]
fn with_display_context_resolved_differs_from_card_name() {
    // The resolved name may differ from the card's display_name (e.g. nickname overrides)
    let contact = make_contact("Robert");
    let mc = MobileContact::with_display_context(
        &contact,
        Some("Bob".to_string()),
        "Bob".to_string(),
        false,
    );
    assert_eq!(
        mc.resolved_display_name, "Bob",
        "resolved_display_name must be the computed nickname, not the card name"
    );
    assert_ne!(
        mc.resolved_display_name,
        contact.display_name(),
        "resolved_display_name must differ from raw card name when nickname overrides it"
    );
}

// @internal
#[test]
fn with_display_context_preserves_contact_id() {
    let contact = make_contact("Bob");
    let expected_id = contact.id().to_string();
    let mc = MobileContact::with_display_context(&contact, None, "Bob".to_string(), false);
    assert_eq!(
        mc.id, expected_id,
        "MobileContact id must be preserved from the underlying Contact"
    );
}
