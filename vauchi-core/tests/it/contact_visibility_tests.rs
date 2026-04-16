// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for contact::visibility
//! Extracted from visibility.rs

use std::collections::HashSet;
use vauchi_core::contact::{FieldVisibility, VisibilityRules};

// @scenario: visibility_control :: New fields default to visible to all contacts
// @internal
#[test]
fn test_default_is_everyone() {
    let rules = VisibilityRules::new();
    assert_eq!(*rules.get("any_field"), FieldVisibility::Everyone);
}

// @scenario: visibility_control :: Show a field only to specific contacts
// @internal
#[test]
fn test_set_contacts_only() {
    let mut rules = VisibilityRules::new();
    let mut allowed = HashSet::new();
    allowed.insert("alice".to_string());
    allowed.insert("bob".to_string());

    rules.set_contacts("phone", allowed);

    assert!(rules.can_see("phone", "alice"));
    assert!(rules.can_see("phone", "bob"));
    assert!(!rules.can_see("phone", "charlie"));
}

// @scenario: visibility_control :: Make a field private (visible to none)
// @internal
#[test]
fn test_set_nobody() {
    let mut rules = VisibilityRules::new();
    rules.set_nobody("secret_field");

    assert!(!rules.can_see("secret_field", "alice"));
    assert!(!rules.can_see("secret_field", "bob"));
}

// @scenario: visibility_control :: Show a field only to specific contacts
// @scenario: visibility_control :: Make a field private (visible to none)
// @scenario: contacts_management :: Contact shows only fields I can see
// @scenario: visibility_control :: View what a specific contact can see
// @internal
#[test]
fn test_visible_fields() {
    let mut rules = VisibilityRules::new();
    rules.set_nobody("private");

    let mut allowed = HashSet::new();
    allowed.insert("alice".to_string());
    rules.set_contacts("restricted", allowed);

    let all_fields = vec!["public", "private", "restricted"];

    let alice_visible = rules.visible_fields("alice", &all_fields);
    assert!(alice_visible.contains(&"public".to_string()));
    assert!(!alice_visible.contains(&"private".to_string()));
    assert!(alice_visible.contains(&"restricted".to_string()));

    let bob_visible = rules.visible_fields("bob", &all_fields);
    assert!(bob_visible.contains(&"public".to_string()));
    assert!(!bob_visible.contains(&"private".to_string()));
    assert!(!bob_visible.contains(&"restricted".to_string()));
}

// ============================================================
// Additional tests (added for coverage)
// ============================================================

// @scenario: visibility_control :: New fields default to visible to all contacts
// @internal
#[test]
fn test_set_everyone_explicit() {
    let mut rules = VisibilityRules::new();

    // First set to nobody
    rules.set_nobody("phone");
    assert!(!rules.can_see("phone", "alice"));

    // Then explicitly set to everyone
    rules.set_everyone("phone");
    assert!(rules.can_see("phone", "alice"));
    assert!(rules.can_see("phone", "anyone"));
}

// @scenario: visibility_control :: Reset all visibility to default
// @internal
#[test]
fn test_remove_reverts_to_default() {
    let mut rules = VisibilityRules::new();

    // Set to nobody
    rules.set_nobody("secret");
    assert!(!rules.can_see("secret", "alice"));

    // Remove rule - should revert to default (everyone)
    rules.remove("secret");
    assert!(rules.can_see("secret", "alice"));
    assert_eq!(*rules.get("secret"), FieldVisibility::Everyone);
}

// @internal
#[test]
fn test_field_visibility_equality() {
    let v1 = FieldVisibility::Everyone;
    let v2 = FieldVisibility::Everyone;
    assert_eq!(v1, v2);

    let v3 = FieldVisibility::Nobody;
    assert_ne!(v1, v3);

    let mut set1 = HashSet::new();
    set1.insert("alice".to_string());
    let mut set2 = HashSet::new();
    set2.insert("alice".to_string());

    let v4 = FieldVisibility::Contacts(set1);
    let v5 = FieldVisibility::Contacts(set2);
    assert_eq!(v4, v5);
}

// @scenario: visibility_control :: Visibility settings persist after app restart
// @internal
#[test]
fn test_visibility_rules_serialization() {
    let mut rules = VisibilityRules::new();
    rules.set_nobody("private");

    let mut allowed = HashSet::new();
    allowed.insert("alice".to_string());
    rules.set_contacts("restricted", allowed);

    // Serialize
    let json = serde_json::to_string(&rules).unwrap();

    // Deserialize
    let restored: VisibilityRules = serde_json::from_str(&json).unwrap();

    assert!(!restored.can_see("private", "anyone"));
    assert!(restored.can_see("restricted", "alice"));
    assert!(!restored.can_see("restricted", "bob"));
}

// @scenario: visibility_control :: Show a field only to specific contacts
// @internal
#[test]
fn test_empty_contacts_set() {
    let mut rules = VisibilityRules::new();

    // Set to empty contacts set - no one can see
    rules.set_contacts("field", HashSet::new());

    assert!(!rules.can_see("field", "alice"));
    assert!(!rules.can_see("field", "bob"));
}

// @scenario: visibility_control :: Show a field only to specific contacts
// @scenario: visibility_control :: Make a field private (visible to none)
// @internal
#[test]
fn test_multiple_rules() {
    let mut rules = VisibilityRules::new();

    // Set different visibility for multiple fields
    rules.set_nobody("private1");
    rules.set_nobody("private2");
    rules.set_everyone("public1");

    let mut allowed = HashSet::new();
    allowed.insert("alice".to_string());
    rules.set_contacts("restricted", allowed);

    // Check all rules work independently
    assert!(!rules.can_see("private1", "alice"));
    assert!(!rules.can_see("private2", "alice"));
    assert!(rules.can_see("public1", "anyone"));
    assert!(rules.can_see("restricted", "alice"));
    assert!(!rules.can_see("restricted", "bob"));
}
