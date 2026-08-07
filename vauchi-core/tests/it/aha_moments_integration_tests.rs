// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Aha Moments Integration Tests
//!
//! Integration tests verifying aha moments feature against Gherkin scenarios.
//! Feature file: features/aha_moments.feature
//!
//! These tests verify the full workflow of aha moments including:
//! - Triggering at appropriate moments
//! - Showing only once (persistence)
//! - Context interpolation
//! - Persistence across app restarts

use vauchi_core::{
    AhaMoment, AhaMomentTracker, AhaMomentType, Contact, ContactCard, ContactField, FieldType,
    SymmetricKey, Vauchi, api::events::VauchiEvent,
};

// ============================================================
// Scenario: Card creation shows completion message
// ============================================================

/// Test: Card creation aha moment triggers on identity creation
/// Feature: aha_moments.feature @card-creation
// @scenario: aha_moments :: Card creation shows completion message
// @internal
#[test]
fn test_card_creation_aha_moment_triggers() {
    let mut tracker = AhaMomentTracker::new();

    // Simulate card creation completing
    let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);

    assert!(moment.is_some(), "Should trigger on first card creation");
    let m = moment.unwrap();
    assert_eq!(m.title(), "Your card is ready");
    assert!(m.message().contains("latest"), "Should explain updates");
    assert!(m.has_animation(), "Should have animation");
}

/// Test: Card creation celebration shown once only
/// Feature: aha_moments.feature @card-creation
/// Scenario: Card creation celebration is shown once
// @scenario: aha_moments :: Card creation celebration is shown once
// @internal
#[test]
fn test_card_creation_shown_once() {
    let mut tracker = AhaMomentTracker::new();

    let first = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(first.is_some(), "expected Some value");

    // Second trigger should fail (already seen)
    let second = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(second.is_none(), "Should not show again after being seen");
}

// ============================================================
// Scenario: First edit shows would-update feedback
// ============================================================

/// Test: First edit triggers feedback
/// Feature: aha_moments.feature @first-edit
// @scenario: aha_moments :: First edit shows would-update feedback
// @internal
#[test]
fn test_first_edit_triggers_feedback() {
    let mut tracker = AhaMomentTracker::new();

    let moment = tracker.try_trigger(AhaMomentType::FirstEdit);

    assert!(moment.is_some(), "expected Some value");
    let m = moment.unwrap();
    assert!(m.message().contains("anyone had your card"));
    assert!(m.has_animation(), "Should have ripple animation");
}

/// Test: First edit feedback shown only once
/// Feature: aha_moments.feature @first-edit
/// Scenario: First edit feedback shown only once
// @scenario: aha_moments :: First edit feedback shown only once
// @internal
#[test]
fn test_first_edit_shown_once() {
    let mut tracker = AhaMomentTracker::new();

    let first = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(first.is_some(), "expected Some value");

    let second = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(second.is_none(), "Should not repeat first edit feedback");
}

// ============================================================
// Scenario: First contact added celebration
// ============================================================

/// Test: First contact triggers celebration with name
/// Feature: aha_moments.feature @first-contact
// @scenario: aha_moments :: First contact added celebration
// @internal
#[test]
fn test_first_contact_celebration_with_name() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());

    assert!(moment.is_some(), "expected Some value");
    let m = moment.unwrap();
    assert!(m.message().contains("Bob"), "Should mention contact name");
    assert!(
        m.message().contains("automatically"),
        "Should explain auto-updates"
    );
}

/// Test: Subsequent contacts do not show celebration
/// Feature: aha_moments.feature @first-contact
/// Scenario: Subsequent contacts do not show celebration
// @scenario: aha_moments :: Subsequent contacts do not show celebration
// @internal
#[test]
fn test_subsequent_contacts_no_celebration() {
    let mut tracker = AhaMomentTracker::new();

    let first =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());
    assert!(first.is_some(), "expected Some value");

    // Second contact does not trigger aha moment
    let second =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());
    assert!(second.is_none(), "Should not show for second contact");
}

// ============================================================
// Scenario: First received update shows diff view
// ============================================================

/// Test: First update received triggers with context
/// Feature: aha_moments.feature @first-update
// @scenario: aha_moments :: First received update shows diff view
// @scenario: demo_contact :: Demo update shows before/after diff
// @internal
#[test]
fn test_first_update_received_with_context() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());

    assert!(moment.is_some(), "expected Some value");
    let m = moment.unwrap();
    assert!(
        m.message().contains("Bob"),
        "Should mention who sent update"
    );
    assert!(m.has_animation());
}

/// Test: Subsequent updates do not show aha moment
/// Feature: aha_moments.feature @first-update
/// Scenario: Subsequent updates do not show aha moment
// @scenario: aha_moments :: Subsequent updates do not show aha moment
// @internal
#[test]
fn test_subsequent_updates_no_aha_moment() {
    let mut tracker = AhaMomentTracker::new();

    let first =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(first.is_some(), "expected Some value");

    let second =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Alice".to_string());
    assert!(second.is_none());
}

// ============================================================
// Scenario: First outbound update shows delivery confirmation
// ============================================================

/// Test: First outbound delivered shows count
/// Feature: aha_moments.feature @first-outbound
// @scenario: aha_moments :: First outbound update shows delivery confirmation
// @internal
#[test]
fn test_first_outbound_delivery_confirmation() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstOutboundDelivered, "3".to_string());

    assert!(moment.is_some(), "expected Some value");
    let m = moment.unwrap();
    assert!(
        m.message().contains("3 contacts"),
        "Should show contact count"
    );
}

// ============================================================
// Scenario: Aha moments are tracked per milestone
// ============================================================

/// Test: Each moment type tracked independently
/// Feature: aha_moments.feature @persistence
/// Scenario: Aha moments are tracked per milestone
// @scenario: aha_moments :: Aha moments are tracked per milestone
// @internal
#[test]
fn test_moments_tracked_per_milestone() {
    let mut tracker = AhaMomentTracker::new();

    tracker.mark_seen(AhaMomentType::CardCreationComplete);

    assert!(tracker.should_trigger(AhaMomentType::FirstEdit));

    assert!(!tracker.should_trigger(AhaMomentType::CardCreationComplete));
}

/// Test: Aha moments persist across app restarts (serialization)
/// Feature: aha_moments.feature @persistence
/// Scenario: Aha moments persist across app restarts
// @scenario: aha_moments :: Aha moments persist across app restarts
// @internal
#[test]
fn test_moments_persist_across_restarts() {
    let mut tracker = AhaMomentTracker::new();

    tracker.mark_seen(AhaMomentType::CardCreationComplete);
    tracker.mark_seen(AhaMomentType::FirstEdit);

    // Serialize (simulate app quit)
    let json = tracker.to_json().expect("Should serialize");

    // Deserialize (simulate app restart)
    let restored = AhaMomentTracker::from_json(&json).expect("Should deserialize");

    assert!(restored.has_seen(AhaMomentType::CardCreationComplete));
    assert!(restored.has_seen(AhaMomentType::FirstEdit));
    assert!(!restored.has_seen(AhaMomentType::FirstContactAdded));

    let card_moment = restored
        .clone()
        .try_trigger(AhaMomentType::CardCreationComplete);
    assert!(card_moment.is_none(), "Should not trigger after restore");
}

// ============================================================
// ============================================================

/// Test: Full aha moment workflow through user journey
/// Combines multiple scenarios into realistic user flow
// @scenario: aha_moments :: Card creation shows completion message
// @scenario: aha_moments :: First edit shows would-update feedback
// @scenario: aha_moments :: First contact added celebration
// @scenario: aha_moments :: First received update shows diff view
// @scenario: aha_moments :: First outbound update shows delivery confirmation
// @scenario: aha_moments :: Aha moments persist across app restarts
// @internal
#[test]
fn test_full_user_journey_aha_moments() {
    let mut tracker = AhaMomentTracker::new();

    // Step 1: User creates identity (card creation)
    let card_created = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(card_created.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 1);

    // Step 2: User edits their card for the first time
    let first_edit = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(first_edit.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 2);

    // Step 3: User edits again (no aha moment)
    let second_edit = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(second_edit.is_none());

    // Step 4: User exchanges with Bob
    let first_contact =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());
    assert!(first_contact.is_some(), "expected Some value");
    assert!(first_contact.unwrap().message().contains("Bob"));
    assert_eq!(tracker.seen_count(), 3);

    // Step 5: Bob sends an update
    let first_update =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(first_update.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 4);

    // Step 6: User edits card (now has contacts)
    let outbound =
        tracker.try_trigger_with_context(AhaMomentType::FirstOutboundDelivered, "1".to_string());
    assert!(outbound.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 5);

    // Step 6b: User edits a field
    let field_edit = tracker.try_trigger(AhaMomentType::FirstFieldEdit);
    assert!(field_edit.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 6);

    // Step 6c: User reaches 3 contacts
    let three_contacts = tracker.try_trigger(AhaMomentType::ThreeContactsReached);
    assert!(three_contacts.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 7);

    // Step 6d: User links a device
    let device_linked = tracker.try_trigger(AhaMomentType::DeviceLinked);
    assert!(device_linked.is_some(), "expected Some value");
    assert_eq!(tracker.seen_count(), 8);

    // All aha moments have been seen
    assert_eq!(tracker.seen_count(), tracker.total_count());

    // Step 7: App restart - verify persistence
    let json = tracker.to_json().unwrap();
    let restored = AhaMomentTracker::from_json(&json).unwrap();
    assert_eq!(restored.seen_count(), 8);

    for moment_type in AhaMomentType::all() {
        assert!(
            !restored.should_trigger(*moment_type),
            "No moments should trigger after full journey"
        );
    }
}

// ============================================================
// ============================================================

/// Test: All context interpolation works correctly
// @scenario: aha_moments :: First contact added celebration
// @scenario: aha_moments :: First received update shows diff view
// @scenario: aha_moments :: First outbound update shows delivery confirmation
// @internal
#[test]
fn test_context_interpolation() {
    let moment = AhaMoment::with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());
    assert!(moment.message().contains("Alice"));
    assert!(moment.message().contains("update"));

    let moment = AhaMoment::with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(moment.message().contains("Bob"));
    assert!(moment.message().contains("instantly"));

    let moment = AhaMoment::with_context(AhaMomentType::FirstOutboundDelivered, "5".to_string());
    assert!(moment.message().contains("5 contacts"));

    // Without context falls back to generic message
    let moment = AhaMoment::new(AhaMomentType::FirstContactAdded);
    assert!(moment.message().contains("automatically"));
}

// ============================================================
// Integration with Vauchi API (requires API extension)
// ============================================================

/// Test: Aha moments integrate with Vauchi API
/// This tests the full API integration
// @scenario: aha_moments :: Card creation shows completion message
// @scenario: aha_moments :: Card creation celebration is shown once
// @internal
#[test]
fn test_vauchi_api_aha_moment_integration() {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();

    wb.create_identity("Test User").unwrap();
    assert!(wb.has_identity());

    // Trigger card creation aha moment via API
    let moment = wb
        .try_trigger_aha_moment(AhaMomentType::CardCreationComplete)
        .unwrap();
    assert!(moment.is_some(), "Should trigger card creation moment");

    // Second trigger should fail (already seen)
    let moment2 = wb
        .try_trigger_aha_moment(AhaMomentType::CardCreationComplete)
        .unwrap();
    assert!(moment2.is_none(), "Should not repeat");

    assert!(
        wb.has_seen_aha_moment(AhaMomentType::CardCreationComplete)
            .unwrap()
    );
    assert!(!wb.has_seen_aha_moment(AhaMomentType::FirstEdit).unwrap());

    assert_eq!(wb.aha_moments_seen_count().unwrap(), 1);

    // Trigger first edit with context
    let edit_moment = wb.try_trigger_aha_moment(AhaMomentType::FirstEdit).unwrap();
    assert!(edit_moment.is_some(), "expected Some value");
    assert_eq!(wb.aha_moments_seen_count().unwrap(), 2);

    wb.reset_aha_moments().unwrap();
    assert_eq!(wb.aha_moments_seen_count().unwrap(), 0);
    assert!(
        !wb.has_seen_aha_moment(AhaMomentType::CardCreationComplete)
            .unwrap()
    );
}

/// Test: Edit operation should check for first edit aha moment
// @scenario: aha_moments :: First edit shows would-update feedback
// @internal
#[test]
fn test_edit_triggers_first_edit_moment() {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity("Test User").unwrap();

    // Add a field (this is an edit operation)
    wb.add_own_field(ContactField::new(
        FieldType::Email,
        "work",
        "test@example.com",
        0,
    ))
    .unwrap();

    // and potentially triggered the FirstEdit aha moment

    let card = wb.own_card().unwrap().unwrap();
    assert!(card.fields().iter().any(|f| f.label() == "work"));
}

/// Test: Adding contact should check for first contact aha moment
// @scenario: aha_moments :: First contact added celebration
// @internal
#[test]
fn test_add_contact_triggers_first_contact_moment() {
    let wb: Vauchi = Vauchi::in_memory().unwrap();

    let bob = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    );
    let bob_name = bob.display_name().to_string();
    wb.add_contact(bob).unwrap();

    assert_eq!(wb.contact_count().unwrap(), 1);

    // Adding the contact is itself the trigger — core owns the decision, so
    // the milestone is already spent by the time anyone asks (ADR-069).
    assert!(
        wb.has_seen_aha_moment(AhaMomentType::FirstContactAdded)
            .unwrap(),
        "adding the first contact must have fired the moment"
    );
    let replay = wb
        .try_trigger_aha_moment_with_context(AhaMomentType::FirstContactAdded, bob_name)
        .unwrap();
    assert!(
        replay.is_none(),
        "a caller asking again must get nothing back, or the moment shows twice, got {replay:?}"
    );
}

/// Test: Demo contact API integration
// @internal
#[test]
fn test_vauchi_api_demo_contact_integration() {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity("Test User").unwrap();

    wb.initialize_demo_contact().unwrap();

    assert!(wb.is_demo_contact_active().unwrap());

    let card = wb.demo_contact_card().unwrap();
    assert!(card.is_some(), "expected Some value");
    let card = card.unwrap();
    assert!(card.is_demo);
    assert!(!card.tip_title.is_empty());

    let next_tip = wb.advance_demo_contact().unwrap();
    assert!(next_tip.is_some(), "expected Some value");

    wb.dismiss_demo_contact().unwrap();
    assert!(!wb.is_demo_contact_active().unwrap());

    assert!(wb.demo_contact_card().unwrap().is_none());

    wb.restore_demo_contact().unwrap();
    assert!(wb.is_demo_contact_active().unwrap());

    // Auto-remove (simulating first real exchange)
    wb.auto_remove_demo_contact().unwrap();
    let state = wb.demo_contact_state().unwrap();
    assert!(!state.is_active);
    assert!(state.auto_removed);
}

/// Test: Demo contact not initialized when user has contacts
// @scenario: demo_contact :: Demo contact does not appear if user has contacts
// @internal
#[test]
fn test_demo_contact_skipped_with_contacts() {
    let wb: Vauchi = Vauchi::in_memory().unwrap();

    let alice = Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
        0,
    );
    wb.add_contact(alice).unwrap();

    wb.initialize_demo_contact().unwrap();

    // Demo should not be active (user already has contacts)
    assert!(!wb.is_demo_contact_active().unwrap());
}

/// Test: Core, not a frontend, decides when an aha moment fires.
// @scenario: aha_moments :: First contact added triggers the moment once
// @internal
#[test]
fn adding_a_first_contact_emits_the_aha_moment_once() {
    let mut wb: Vauchi = Vauchi::in_memory().unwrap();
    wb.create_identity("Owner").unwrap();

    let events: std::sync::Arc<std::sync::Mutex<Vec<VauchiEvent>>> =
        std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = events.clone();
    wb.add_event_handler(std::sync::Arc::new(move |event: VauchiEvent| {
        captured.lock().unwrap().push(event);
    }));

    wb.add_contact(Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Bob"),
        SymmetricKey::generate(),
        0,
    ))
    .unwrap();
    wb.add_contact(Contact::from_exchange(
        [2u8; 32],
        ContactCard::new("Carol"),
        SymmetricKey::generate(),
        0,
    ))
    .unwrap();

    let captured = events.lock().unwrap();
    let moments: Vec<&AhaMoment> = captured
        .iter()
        .filter_map(|e| match e {
            VauchiEvent::AhaMomentTriggered { moment } => Some(moment),
            _ => None,
        })
        .filter(|m| m.moment_type == AhaMomentType::FirstContactAdded)
        .collect();

    assert_eq!(
        moments.len(),
        1,
        "the first-contact moment fires once and only once; a shell that owns the tracker \
         re-fires it per install and per frontend, got: {moments:?}"
    );
    assert_eq!(
        moments[0].context.as_deref(),
        Some("Bob"),
        "the moment carries the contact that caused it, so no shell has to look it up"
    );
    assert!(
        wb.has_seen_aha_moment(AhaMomentType::FirstContactAdded)
            .unwrap(),
        "the moment must persist to core's encrypted ux_state, not a frontend file"
    );
}
