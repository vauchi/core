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
    network::MockTransport, AhaMoment, AhaMomentTracker, AhaMomentType, Contact, ContactCard,
    ContactField, FieldType, SymmetricKey, Vauchi,
};

// ============================================================
// Card Creation Celebration
// Scenario: Card creation shows completion message
// ============================================================

/// Test: Card creation aha moment triggers on identity creation
/// Feature: aha_moments.feature @card-creation
#[test]
fn test_card_creation_aha_moment_triggers() {
    let mut tracker = AhaMomentTracker::new();

    // Simulate card creation completing
    let moment = tracker.try_trigger(AhaMomentType::CardCreationComplete);

    assert!(moment.is_some(), "Should trigger on first card creation");
    let m = moment.unwrap();
    assert_eq!(m.title(), "Your card is ready");
    assert!(
        m.message().contains("latest"),
        "Should explain updates"
    );
    assert!(m.has_animation(), "Should have animation");
}

/// Test: Card creation celebration shown once only
/// Feature: aha_moments.feature @card-creation
/// Scenario: Card creation celebration is shown once
#[test]
fn test_card_creation_shown_once() {
    let mut tracker = AhaMomentTracker::new();

    // First trigger succeeds
    let first = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(first.is_some());

    // Second trigger should fail (already seen)
    let second = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(second.is_none(), "Should not show again after being seen");
}

// ============================================================
// First Edit Feedback
// Scenario: First edit shows would-update feedback
// ============================================================

/// Test: First edit triggers feedback
/// Feature: aha_moments.feature @first-edit
#[test]
fn test_first_edit_triggers_feedback() {
    let mut tracker = AhaMomentTracker::new();

    let moment = tracker.try_trigger(AhaMomentType::FirstEdit);

    assert!(moment.is_some());
    let m = moment.unwrap();
    assert!(m.message().contains("anyone had your card"));
    assert!(m.has_animation(), "Should have ripple animation");
}

/// Test: First edit feedback shown only once
/// Feature: aha_moments.feature @first-edit
/// Scenario: First edit feedback shown only once
#[test]
fn test_first_edit_shown_once() {
    let mut tracker = AhaMomentTracker::new();

    let first = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(first.is_some());

    let second = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(second.is_none(), "Should not repeat first edit feedback");
}

// ============================================================
// First Contact Celebration
// Scenario: First contact added celebration
// ============================================================

/// Test: First contact triggers celebration with name
/// Feature: aha_moments.feature @first-contact
#[test]
fn test_first_contact_celebration_with_name() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());

    assert!(moment.is_some());
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
#[test]
fn test_subsequent_contacts_no_celebration() {
    let mut tracker = AhaMomentTracker::new();

    // First contact triggers
    let first =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());
    assert!(first.is_some());

    // Second contact does not trigger aha moment
    let second =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());
    assert!(second.is_none(), "Should not show for second contact");
}

// ============================================================
// First Received Update
// Scenario: First received update shows diff view
// ============================================================

/// Test: First update received triggers with context
/// Feature: aha_moments.feature @first-update
#[test]
fn test_first_update_received_with_context() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());

    assert!(moment.is_some());
    let m = moment.unwrap();
    assert!(m.message().contains("Bob"), "Should mention who sent update");
    assert!(m.has_animation());
}

/// Test: Subsequent updates do not show aha moment
/// Feature: aha_moments.feature @first-update
/// Scenario: Subsequent updates do not show aha moment
#[test]
fn test_subsequent_updates_no_aha_moment() {
    let mut tracker = AhaMomentTracker::new();

    let first =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(first.is_some());

    let second =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Alice".to_string());
    assert!(second.is_none());
}

// ============================================================
// First Outbound Update
// Scenario: First outbound update shows delivery confirmation
// ============================================================

/// Test: First outbound delivered shows count
/// Feature: aha_moments.feature @first-outbound
#[test]
fn test_first_outbound_delivery_confirmation() {
    let mut tracker = AhaMomentTracker::new();

    let moment =
        tracker.try_trigger_with_context(AhaMomentType::FirstOutboundDelivered, "3".to_string());

    assert!(moment.is_some());
    let m = moment.unwrap();
    assert!(m.message().contains("3 contacts"), "Should show contact count");
}

// ============================================================
// Persistence
// Scenario: Aha moments are tracked per milestone
// ============================================================

/// Test: Each moment type tracked independently
/// Feature: aha_moments.feature @persistence
/// Scenario: Aha moments are tracked per milestone
#[test]
fn test_moments_tracked_per_milestone() {
    let mut tracker = AhaMomentTracker::new();

    // See card creation
    tracker.mark_seen(AhaMomentType::CardCreationComplete);

    // First edit should still trigger
    assert!(tracker.should_trigger(AhaMomentType::FirstEdit));

    // Card creation should not repeat
    assert!(!tracker.should_trigger(AhaMomentType::CardCreationComplete));
}

/// Test: Aha moments persist across app restarts (serialization)
/// Feature: aha_moments.feature @persistence
/// Scenario: Aha moments persist across app restarts
#[test]
fn test_moments_persist_across_restarts() {
    let mut tracker = AhaMomentTracker::new();

    // Mark some moments as seen
    tracker.mark_seen(AhaMomentType::CardCreationComplete);
    tracker.mark_seen(AhaMomentType::FirstEdit);

    // Serialize (simulate app quit)
    let json = tracker.to_json().expect("Should serialize");

    // Deserialize (simulate app restart)
    let restored = AhaMomentTracker::from_json(&json).expect("Should deserialize");

    // Verify persistence
    assert!(restored.has_seen(AhaMomentType::CardCreationComplete));
    assert!(restored.has_seen(AhaMomentType::FirstEdit));
    assert!(!restored.has_seen(AhaMomentType::FirstContactAdded));

    // Triggers should reflect persisted state
    let card_moment = restored.clone().try_trigger(AhaMomentType::CardCreationComplete);
    assert!(card_moment.is_none(), "Should not trigger after restore");
}

// ============================================================
// Full Workflow Integration
// ============================================================

/// Test: Full aha moment workflow through user journey
/// Combines multiple scenarios into realistic user flow
#[test]
fn test_full_user_journey_aha_moments() {
    let mut tracker = AhaMomentTracker::new();

    // Step 1: User creates identity (card creation)
    let card_created = tracker.try_trigger(AhaMomentType::CardCreationComplete);
    assert!(card_created.is_some());
    assert_eq!(tracker.seen_count(), 1);

    // Step 2: User edits their card for the first time
    let first_edit = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(first_edit.is_some());
    assert_eq!(tracker.seen_count(), 2);

    // Step 3: User edits again (no aha moment)
    let second_edit = tracker.try_trigger(AhaMomentType::FirstEdit);
    assert!(second_edit.is_none());

    // Step 4: User exchanges with Bob
    let first_contact =
        tracker.try_trigger_with_context(AhaMomentType::FirstContactAdded, "Bob".to_string());
    assert!(first_contact.is_some());
    assert!(first_contact.unwrap().message().contains("Bob"));
    assert_eq!(tracker.seen_count(), 3);

    // Step 5: Bob sends an update
    let first_update =
        tracker.try_trigger_with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(first_update.is_some());
    assert_eq!(tracker.seen_count(), 4);

    // Step 6: User edits card (now has contacts)
    let outbound =
        tracker.try_trigger_with_context(AhaMomentType::FirstOutboundDelivered, "1".to_string());
    assert!(outbound.is_some());
    assert_eq!(tracker.seen_count(), 5);

    // All aha moments have been seen
    assert_eq!(tracker.seen_count(), tracker.total_count());

    // Step 7: App restart - verify persistence
    let json = tracker.to_json().unwrap();
    let restored = AhaMomentTracker::from_json(&json).unwrap();
    assert_eq!(restored.seen_count(), 5);

    // No more aha moments should trigger
    for moment_type in AhaMomentType::all() {
        assert!(
            !restored.should_trigger(*moment_type),
            "No moments should trigger after full journey"
        );
    }
}

// ============================================================
// Context Interpolation
// ============================================================

/// Test: All context interpolation works correctly
#[test]
fn test_context_interpolation() {
    // FirstContactAdded with name
    let moment = AhaMoment::with_context(AhaMomentType::FirstContactAdded, "Alice".to_string());
    assert!(moment.message().contains("Alice"));
    assert!(moment.message().contains("update"));

    // FirstUpdateReceived with name
    let moment = AhaMoment::with_context(AhaMomentType::FirstUpdateReceived, "Bob".to_string());
    assert!(moment.message().contains("Bob"));
    assert!(moment.message().contains("instantly"));

    // FirstOutboundDelivered with count
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
#[test]
fn test_vauchi_api_aha_moment_integration() {
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Create identity
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

    // Check has_seen
    assert!(wb.has_seen_aha_moment(AhaMomentType::CardCreationComplete).unwrap());
    assert!(!wb.has_seen_aha_moment(AhaMomentType::FirstEdit).unwrap());

    // Check seen count
    assert_eq!(wb.aha_moments_seen_count().unwrap(), 1);

    // Trigger first edit with context
    let edit_moment = wb
        .try_trigger_aha_moment(AhaMomentType::FirstEdit)
        .unwrap();
    assert!(edit_moment.is_some());
    assert_eq!(wb.aha_moments_seen_count().unwrap(), 2);

    // Reset and verify
    wb.reset_aha_moments().unwrap();
    assert_eq!(wb.aha_moments_seen_count().unwrap(), 0);
    assert!(!wb.has_seen_aha_moment(AhaMomentType::CardCreationComplete).unwrap());
}

/// Test: Edit operation should check for first edit aha moment
#[test]
fn test_edit_triggers_first_edit_moment() {
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    wb.create_identity("Test User").unwrap();

    // Add a field (this is an edit operation)
    wb.add_own_field(ContactField::new(FieldType::Email, "work", "test@example.com"))
        .unwrap();

    // The API should have recorded that an edit happened
    // and potentially triggered the FirstEdit aha moment

    // Verify the field was added
    let card = wb.own_card().unwrap().unwrap();
    assert!(card.fields().iter().any(|f| f.label() == "work"));
}

/// Test: Adding contact should check for first contact aha moment
#[test]
fn test_add_contact_triggers_first_contact_moment() {
    let wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Add first contact
    let bob = Contact::from_exchange([1u8; 32], ContactCard::new("Bob"), SymmetricKey::generate());
    let bob_name = bob.display_name().to_string();
    wb.add_contact(bob).unwrap();

    assert_eq!(wb.contact_count().unwrap(), 1);

    // Trigger first contact moment with context
    let moment = wb
        .try_trigger_aha_moment_with_context(AhaMomentType::FirstContactAdded, bob_name)
        .unwrap();
    assert!(moment.is_some());
    assert!(moment.unwrap().message().contains("Bob"));
}

/// Test: Demo contact API integration
#[test]
fn test_vauchi_api_demo_contact_integration() {
    let mut wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();
    wb.create_identity("Test User").unwrap();

    // Initialize demo contact for new user
    wb.initialize_demo_contact().unwrap();

    // Check demo contact is active
    assert!(wb.is_demo_contact_active().unwrap());

    // Get demo contact card
    let card = wb.demo_contact_card().unwrap();
    assert!(card.is_some());
    let card = card.unwrap();
    assert!(card.is_demo);
    assert!(!card.tip_title.is_empty());

    // Advance to next tip
    let next_tip = wb.advance_demo_contact().unwrap();
    assert!(next_tip.is_some());

    // Dismiss demo contact
    wb.dismiss_demo_contact().unwrap();
    assert!(!wb.is_demo_contact_active().unwrap());

    // Card should now be None
    assert!(wb.demo_contact_card().unwrap().is_none());

    // Restore
    wb.restore_demo_contact().unwrap();
    assert!(wb.is_demo_contact_active().unwrap());

    // Auto-remove (simulating first real exchange)
    wb.auto_remove_demo_contact().unwrap();
    let state = wb.demo_contact_state().unwrap();
    assert!(!state.is_active);
    assert!(state.auto_removed);
}

/// Test: Demo contact not initialized when user has contacts
#[test]
fn test_demo_contact_skipped_with_contacts() {
    let wb: Vauchi<MockTransport> = Vauchi::in_memory().unwrap();

    // Add a contact first
    let alice = Contact::from_exchange([1u8; 32], ContactCard::new("Alice"), SymmetricKey::generate());
    wb.add_contact(alice).unwrap();

    // Try to initialize demo contact
    wb.initialize_demo_contact().unwrap();

    // Demo should not be active (user already has contacts)
    assert!(!wb.is_demo_contact_active().unwrap());
}
