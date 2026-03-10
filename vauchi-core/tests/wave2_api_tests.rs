// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Wave 2 core API additions.
//!
//! Covers:
//! - Duplicate detection and merge APIs
//! - Setup/onboarding progress API
//! - Emergency wipe status and perform APIs
//! - New aha moment types
//! - Label members (get_group_members) API

mod common;

use vauchi_core::network::MockTransport;
use vauchi_core::{
    AhaMomentType, Contact, ContactCard, ContactField, FieldType, SymmetricKey, Vauchi,
};

fn create_test_vauchi() -> Vauchi<MockTransport> {
    Vauchi::in_memory().unwrap()
}

fn create_vauchi_with_identity(name: &str) -> Vauchi<MockTransport> {
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    wb
}

fn create_test_contact(name: &str, pk: [u8; 32]) -> Contact {
    let card = ContactCard::new(name);
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key)
}

fn create_test_contact_with_fields(
    name: &str,
    pk: [u8; 32],
    fields: Vec<(FieldType, &str, &str)>,
) -> Contact {
    let mut card = ContactCard::new(name);
    for (ft, label, value) in fields {
        card.add_field(ContactField::new(ft, label, value)).unwrap();
    }
    let shared_key = SymmetricKey::generate();
    Contact::from_exchange(pk, card, shared_key)
}

// ================================================================
// Duplicate Detection API Tests
// ================================================================

#[test]
fn test_find_duplicates_empty_contacts() {
    let wb = create_vauchi_with_identity("Alice");
    let duplicates = wb.find_duplicates().unwrap();
    assert_eq!(duplicates.len(), 0);
}

#[test]
fn test_find_duplicates_no_duplicates() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_contact(create_test_contact("Bob", [1u8; 32]))
        .unwrap();
    wb.add_contact(create_test_contact("Charlie", [2u8; 32]))
        .unwrap();

    let duplicates = wb.find_duplicates().unwrap();
    assert_eq!(duplicates.len(), 0);
}

#[test]
fn test_find_duplicates_detects_similar_names() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_contact(create_test_contact("Bob Smith", [1u8; 32]))
        .unwrap();
    wb.add_contact(create_test_contact("Bob Smith", [2u8; 32]))
        .unwrap();

    let duplicates = wb.find_duplicates().unwrap();
    assert!(
        !duplicates.is_empty(),
        "should detect contacts with identical names as duplicates"
    );
    assert!(
        duplicates[0].similarity >= 0.7,
        "identical names should have similarity >= 0.7, got {}",
        duplicates[0].similarity
    );
}

#[test]
fn test_get_duplicate_score_returns_similarity() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob Smith", [1u8; 32]);
    let c2 = create_test_contact("Bob Smith", [2u8; 32]);
    let id1 = c1.id().to_string();
    let id2 = c2.id().to_string();
    wb.add_contact(c1).unwrap();
    wb.add_contact(c2).unwrap();

    let score = wb.get_duplicate_score(&id1, &id2).unwrap();
    assert!(
        score >= 0.7,
        "identical names should score >= 0.7, got {}",
        score
    );
}

#[test]
fn test_get_duplicate_score_contact_not_found() {
    let wb = create_vauchi_with_identity("Alice");
    let result = wb.get_duplicate_score("nonexistent1", "nonexistent2");
    assert!(result.is_err());
}

#[test]
fn test_dismiss_duplicate_removes_from_results() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob Smith", [1u8; 32]);
    let c2 = create_test_contact("Bob Smith", [2u8; 32]);
    let id1 = c1.id().to_string();
    let id2 = c2.id().to_string();
    wb.add_contact(c1).unwrap();
    wb.add_contact(c2).unwrap();

    // Before dismissal
    let before = wb.find_duplicates().unwrap();
    assert!(
        !before.is_empty(),
        "should find duplicates before dismissal"
    );

    // Dismiss the pair
    wb.dismiss_duplicate(&id1, &id2).unwrap();

    // After dismissal
    let after = wb.find_duplicates().unwrap();
    assert_eq!(
        after.len(),
        0,
        "dismissed duplicates should not appear in results"
    );
}

#[test]
fn test_dismiss_duplicate_order_independent() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob Smith", [1u8; 32]);
    let c2 = create_test_contact("Bob Smith", [2u8; 32]);
    let id1 = c1.id().to_string();
    let id2 = c2.id().to_string();
    wb.add_contact(c1).unwrap();
    wb.add_contact(c2).unwrap();

    // Dismiss in reverse order
    wb.dismiss_duplicate(&id2, &id1).unwrap();

    let after = wb.find_duplicates().unwrap();
    assert_eq!(
        after.len(),
        0,
        "dismissal should be order-independent via normalization"
    );
}

#[test]
fn test_merge_contacts_combines_fields() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact_with_fields(
        "Bob",
        [1u8; 32],
        vec![(FieldType::Email, "email", "bob@example.com")],
    );
    let c2 = create_test_contact_with_fields(
        "Bob",
        [2u8; 32],
        vec![(FieldType::Phone, "phone", "+1234567890")],
    );
    let primary_id = c1.id().to_string();
    let secondary_id = c2.id().to_string();
    wb.add_contact(c1).unwrap();
    wb.add_contact(c2).unwrap();

    let merged = wb.merge_contacts(&primary_id, &secondary_id).unwrap();

    // Merged contact should have both fields
    assert_eq!(merged.card().fields().len(), 2);
    assert!(merged.card().fields().iter().any(|f| f.label() == "email"));
    assert!(merged.card().fields().iter().any(|f| f.label() == "phone"));

    // Primary still exists
    assert!(wb.get_contact(&primary_id).unwrap().is_some());

    // Secondary should be deleted
    assert!(wb.get_contact(&secondary_id).unwrap().is_none());
}

#[test]
fn test_merge_contacts_preserves_primary_name() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob Primary", [1u8; 32]);
    let c2 = create_test_contact("Bob Secondary", [2u8; 32]);
    let primary_id = c1.id().to_string();
    let secondary_id = c2.id().to_string();
    wb.add_contact(c1).unwrap();
    wb.add_contact(c2).unwrap();

    let merged = wb.merge_contacts(&primary_id, &secondary_id).unwrap();
    assert_eq!(merged.display_name(), "Bob Primary");
}

#[test]
fn test_merge_contacts_not_found_error() {
    let wb = create_vauchi_with_identity("Alice");
    let c1 = create_test_contact("Bob", [1u8; 32]);
    let primary_id = c1.id().to_string();
    wb.add_contact(c1).unwrap();

    let result = wb.merge_contacts(&primary_id, "nonexistent");
    assert!(result.is_err());
}

// ================================================================
// Setup Progress / Onboarding API Tests
// ================================================================

#[test]
fn test_setup_progress_fresh_instance() {
    let wb = create_test_vauchi();
    let progress = wb.get_setup_progress().unwrap();

    assert!(!progress.identity_created);
    assert!(!progress.card_has_fields);
    assert!(!progress.has_contacts);
    assert!(!progress.has_three_contacts);
    assert!(!progress.device_linked);
    assert!(!progress.password_set);
    assert_eq!(progress.completed_steps, 0);
    assert_eq!(progress.total_steps, 6);
    assert!(!progress.is_complete());
    assert_eq!(progress.completion_fraction(), 0.0);
}

#[test]
fn test_setup_progress_after_identity() {
    let wb = create_vauchi_with_identity("Alice");
    let progress = wb.get_setup_progress().unwrap();

    assert!(progress.identity_created);
    assert!(!progress.card_has_fields);
    assert!(!progress.has_contacts);
    assert_eq!(progress.completed_steps, 1);
}

#[test]
fn test_setup_progress_after_adding_field() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ))
    .unwrap();

    let progress = wb.get_setup_progress().unwrap();

    assert!(progress.identity_created);
    assert!(progress.card_has_fields);
    assert_eq!(progress.completed_steps, 2);
}

#[test]
fn test_setup_progress_after_adding_contacts() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_contact(create_test_contact("Bob", [1u8; 32]))
        .unwrap();

    let progress = wb.get_setup_progress().unwrap();
    assert!(progress.has_contacts);
    assert!(!progress.has_three_contacts);

    wb.add_contact(create_test_contact("Charlie", [2u8; 32]))
        .unwrap();
    wb.add_contact(create_test_contact("Dave", [3u8; 32]))
        .unwrap();

    let progress = wb.get_setup_progress().unwrap();
    assert!(progress.has_three_contacts);
}

#[test]
fn test_setup_progress_completion_fraction() {
    let wb = create_vauchi_with_identity("Alice");
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ))
    .unwrap();

    let progress = wb.get_setup_progress().unwrap();
    // 2 out of 6 steps completed
    let expected = 2.0 / 6.0;
    assert!(
        (progress.completion_fraction() - expected).abs() < 0.001,
        "expected ~{:.3}, got {:.3}",
        expected,
        progress.completion_fraction()
    );
}

#[test]
fn test_is_first_launch_true_for_fresh_instance() {
    let wb = create_test_vauchi();
    assert!(wb.is_first_launch().unwrap());
}

#[test]
fn test_is_first_launch_false_after_identity() {
    let wb = create_vauchi_with_identity("Alice");
    assert!(!wb.is_first_launch().unwrap());
}

// ================================================================
// Emergency Wipe Status API Tests
// ================================================================

#[test]
fn test_emergency_wipe_status_unconfigured() {
    let wb = create_vauchi_with_identity("Alice");
    let status = wb.get_emergency_wipe_status().unwrap();

    assert!(!status.broadcast_configured);
    assert!(!status.duress_configured);
    assert!(!status.deletion_scheduled);
    assert!(!status.deletion_executed);
    assert!(!status.has_trusted_contacts);
    assert_eq!(status.trusted_contact_count, 0);
    assert!(!status.password_enabled);
}

#[test]
fn test_emergency_wipe_status_with_emergency_config() {
    let mut wb = create_vauchi_with_identity("Alice");

    let contact = create_test_contact("Bob", [1u8; 32]);
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();

    wb.configure_emergency_broadcast(vec![contact_id], "Help me!".to_string(), false)
        .unwrap();

    let status = wb.get_emergency_wipe_status().unwrap();
    assert!(status.broadcast_configured);
}

#[test]
fn test_emergency_wipe_status_with_trusted_contacts() {
    let wb = create_vauchi_with_identity("Alice");

    let mut contact = create_test_contact("Bob", [1u8; 32]);
    contact.trust_for_recovery();
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    // Must re-save to persist the trust flag
    let _c = wb.get_contact(&contact_id).unwrap().unwrap();
    // Trust is set on the in-memory object but we added it trusted
    // Let's verify via the status
    let status = wb.get_emergency_wipe_status().unwrap();
    assert!(status.has_trusted_contacts);
    assert_eq!(status.trusted_contact_count, 1);
}

#[test]
fn test_perform_emergency_wipe_requires_confirmation() {
    let mut wb = create_vauchi_with_identity("Alice");

    let result = wb.perform_emergency_wipe(false);
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("requires explicit confirmation"));
}

#[test]
fn test_perform_emergency_wipe_clears_contacts() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.add_contact(create_test_contact("Bob", [1u8; 32]))
        .unwrap();
    wb.add_contact(create_test_contact("Charlie", [2u8; 32]))
        .unwrap();

    assert_eq!(wb.contact_count().unwrap(), 2);

    wb.perform_emergency_wipe(true).unwrap();

    assert_eq!(wb.contact_count().unwrap(), 0);
    assert!(!wb.has_identity());
}

#[test]
fn test_perform_emergency_wipe_clears_own_card() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "email",
        "alice@example.com",
    ))
    .unwrap();

    wb.perform_emergency_wipe(true).unwrap();

    let card = wb.own_card().unwrap().unwrap();
    assert_eq!(card.fields().len(), 0);
}

// ================================================================
// New Aha Moment Types Tests
// ================================================================

#[test]
fn test_new_aha_moments_exist() {
    let all = AhaMomentType::all();
    assert_eq!(all.len(), 8, "should have 8 aha moment types");

    // Verify the new types are in the list
    assert!(all.contains(&AhaMomentType::FirstFieldEdit));
    assert!(all.contains(&AhaMomentType::ThreeContactsReached));
    assert!(all.contains(&AhaMomentType::DeviceLinked));
}

#[test]
fn test_first_field_edit_moment_has_content() {
    assert!(!AhaMomentType::FirstFieldEdit.title().is_empty());
    assert!(!AhaMomentType::FirstFieldEdit.message().is_empty());
    assert_eq!(AhaMomentType::FirstFieldEdit.title(), "Field updated!");
}

#[test]
fn test_three_contacts_reached_moment_has_content() {
    assert!(!AhaMomentType::ThreeContactsReached.title().is_empty());
    assert!(!AhaMomentType::ThreeContactsReached.message().is_empty());
    assert_eq!(
        AhaMomentType::ThreeContactsReached.title(),
        "Growing network!"
    );
}

#[test]
fn test_device_linked_moment_has_content() {
    assert!(!AhaMomentType::DeviceLinked.title().is_empty());
    assert!(!AhaMomentType::DeviceLinked.message().is_empty());
    assert_eq!(AhaMomentType::DeviceLinked.title(), "Device linked!");
}

#[test]
fn test_new_aha_moments_trigger_via_api() {
    let wb = create_vauchi_with_identity("Alice");

    // FirstFieldEdit should trigger once
    let moment = wb
        .try_trigger_aha_moment(AhaMomentType::FirstFieldEdit)
        .unwrap();
    assert!(moment.is_some());
    assert_eq!(moment.unwrap().moment_type, AhaMomentType::FirstFieldEdit);

    // Second trigger should return None (already seen)
    let moment = wb
        .try_trigger_aha_moment(AhaMomentType::FirstFieldEdit)
        .unwrap();
    assert!(moment.is_none());
}

#[test]
fn test_new_aha_moments_animations() {
    assert!(AhaMomentType::FirstFieldEdit.has_animation());
    assert!(AhaMomentType::ThreeContactsReached.has_animation());
    assert!(AhaMomentType::DeviceLinked.has_animation());
}

// ================================================================
// Label Members (get_group_members) API Tests
// ================================================================

#[test]
fn test_get_label_members_empty_label() {
    let wb = create_vauchi_with_identity("Alice");
    let label = wb.create_group("Friends").unwrap();

    let members = wb.get_group_members(label.id()).unwrap();
    assert_eq!(members.len(), 0);
}

#[test]
fn test_get_label_members_returns_contacts() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob", [1u8; 32]);
    let c1_id = c1.id().to_string();
    wb.add_contact(c1).unwrap();

    let c2 = create_test_contact("Charlie", [2u8; 32]);
    let c2_id = c2.id().to_string();
    wb.add_contact(c2).unwrap();

    let label = wb.create_group("Friends").unwrap();
    let label_id = label.id().to_string();

    wb.add_contact_to_group(&label_id, &c1_id).unwrap();
    wb.add_contact_to_group(&label_id, &c2_id).unwrap();

    let members = wb.get_group_members(&label_id).unwrap();
    assert_eq!(members.len(), 2);

    let member_names: Vec<&str> = members.iter().map(|c| c.display_name()).collect();
    assert!(member_names.contains(&"Bob"));
    assert!(member_names.contains(&"Charlie"));
}

#[test]
fn test_get_label_members_skips_deleted_contacts() {
    let wb = create_vauchi_with_identity("Alice");

    let c1 = create_test_contact("Bob", [1u8; 32]);
    let c1_id = c1.id().to_string();
    wb.add_contact(c1).unwrap();

    let label = wb.create_group("Friends").unwrap();
    let label_id = label.id().to_string();

    wb.add_contact_to_group(&label_id, &c1_id).unwrap();
    // Also add a non-existent contact ID to the label
    // (this simulates a deleted contact that was in the label)
    wb.add_contact_to_group(&label_id, "nonexistent-id")
        .unwrap();

    let members = wb.get_group_members(&label_id).unwrap();
    assert_eq!(
        members.len(),
        1,
        "should skip nonexistent contacts silently"
    );
    assert_eq!(members[0].display_name(), "Bob");
}

// ================================================================
// Label Display Name Override API Tests
// ================================================================

#[test]
fn test_set_label_display_name_override_api() {
    let wb = create_vauchi_with_identity("Matthew Egloff");
    let label = wb.create_group("Friends").unwrap();

    wb.set_group_display_name_override(label.id(), Some("Matt"))
        .unwrap();

    let loaded = wb.get_group(label.id()).unwrap();
    assert_eq!(loaded.display_name_override(), Some("Matt"));
    assert_eq!(loaded.resolve_display_name("Matthew Egloff"), "Matt");
}

#[test]
fn test_clear_label_display_name_override_api() {
    let wb = create_vauchi_with_identity("Matthew Egloff");
    let label = wb.create_group("Friends").unwrap();

    wb.set_group_display_name_override(label.id(), Some("Matt"))
        .unwrap();
    wb.set_group_display_name_override(label.id(), None)
        .unwrap();

    let loaded = wb.get_group(label.id()).unwrap();
    assert_eq!(loaded.display_name_override(), None);
    assert_eq!(
        loaded.resolve_display_name("Matthew Egloff"),
        "Matthew Egloff"
    );
}

#[test]
fn test_set_label_display_name_override_empty_rejected() {
    let wb = create_vauchi_with_identity("Alice");
    let label = wb.create_group("Work").unwrap();

    let result = wb.set_group_display_name_override(label.id(), Some(""));
    assert!(result.is_err(), "empty override should be rejected");
}

#[test]
fn test_set_label_display_name_override_whitespace_rejected() {
    let wb = create_vauchi_with_identity("Alice");
    let label = wb.create_group("Work").unwrap();

    let result = wb.set_group_display_name_override(label.id(), Some("   "));
    assert!(
        result.is_err(),
        "whitespace-only override should be rejected"
    );
}

#[test]
fn test_set_label_display_name_override_nonexistent_label() {
    let wb = create_vauchi_with_identity("Alice");

    let result = wb.set_group_display_name_override("nonexistent-id", Some("Matt"));
    assert!(result.is_err(), "nonexistent label should fail");
}
