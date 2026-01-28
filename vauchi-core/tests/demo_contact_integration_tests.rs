// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Demo Contact Integration Tests
//!
//! Integration tests verifying demo contact feature against Gherkin scenarios.
//! Feature file: features/demo_contact.feature
//!
//! These tests verify the demo contact workflow including:
//! - Appearance for solo users
//! - Periodic updates with rotating tips
//! - Dismissal and auto-removal
//! - Persistence across restarts

use vauchi_core::{
    generate_demo_contact_card, get_demo_tips, DemoContactState, DemoTipCategory, DEMO_CONTACT_ID,
    DEMO_CONTACT_NAME,
};

// ============================================================
// Demo Contact Appearance
// Scenario: Demo contact appears for users with no contacts
// ============================================================

/// Test: Demo contact has correct name and ID
/// Feature: demo_contact.feature @demo-appear
#[test]
fn test_demo_contact_constants() {
    assert_eq!(DEMO_CONTACT_NAME, "Vauchi Tips");
    assert_eq!(DEMO_CONTACT_ID, "demo-vauchi-tips");
}

/// Test: Demo contact appears as active for new users
/// Feature: demo_contact.feature @demo-appear
#[test]
fn test_demo_contact_appears_for_new_users() {
    let state = DemoContactState::new_active();

    assert!(state.is_active, "Should be active for new users");
    assert!(!state.was_dismissed, "Should not be dismissed initially");
    assert!(!state.auto_removed, "Should not be auto-removed initially");
}

/// Test: Demo contact is visually distinct (marked as demo)
/// Feature: demo_contact.feature @demo-appear
#[test]
fn test_demo_contact_visually_distinct() {
    let state = DemoContactState::new_active();
    let tip = state.current_tip().unwrap();
    let card = generate_demo_contact_card(&tip);

    assert!(card.is_demo, "Card should be marked as demo");
    assert_eq!(card.display_name, DEMO_CONTACT_NAME);
    assert_eq!(card.id, DEMO_CONTACT_ID);
}

// ============================================================
// Demo Updates
// Scenario: Demo contact sends periodic updates
// ============================================================

/// Test: Demo tips exist and have content
/// Feature: demo_contact.feature @demo-updates
#[test]
fn test_demo_tips_exist() {
    let tips = get_demo_tips();

    assert!(!tips.is_empty(), "Should have demo tips");
    assert!(tips.len() >= 5, "Should have at least 5 tips");

    // Each tip should have content
    for tip in &tips {
        assert!(!tip.id.is_empty(), "Tip should have ID");
        assert!(!tip.title.is_empty(), "Tip should have title");
        assert!(!tip.content.is_empty(), "Tip should have content");
    }
}

/// Test: Demo state tracks current tip
/// Feature: demo_contact.feature @demo-updates
#[test]
fn test_demo_state_tracks_tip() {
    let state = DemoContactState::new_active();

    let tip = state.current_tip();
    assert!(tip.is_some(), "Should have a current tip");
}

/// Test: Tips advance over time
/// Feature: demo_contact.feature @demo-updates
#[test]
fn test_demo_tips_advance() {
    let mut state = DemoContactState::new_active();
    let initial_index = state.current_tip_index;

    let next_tip = state.advance_to_next_tip();
    assert!(next_tip.is_some());
    assert_ne!(
        state.current_tip_index, initial_index,
        "Index should advance"
    );
    assert_eq!(state.update_count, 1, "Update count should increment");
}

/// Test: Update due check respects interval
/// Feature: demo_contact.feature @demo-updates
#[test]
fn test_demo_update_due_check() {
    let state = DemoContactState::new_active();

    // Just created, update not due yet
    assert!(
        !state.is_update_due(),
        "Update should not be due immediately"
    );
}

// ============================================================
// Demo Contact Content
// Scenario: Demo contact has rotating tips
// ============================================================

/// Test: Tips cover multiple categories
/// Feature: demo_contact.feature @demo-content
#[test]
fn test_demo_tips_multiple_categories() {
    let tips = get_demo_tips();
    let categories: std::collections::HashSet<_> = tips.iter().map(|t| t.category).collect();

    // Check for expected categories
    assert!(
        categories.contains(&DemoTipCategory::GettingStarted),
        "Should have getting started tips"
    );
    assert!(
        categories.contains(&DemoTipCategory::Privacy),
        "Should have privacy tips"
    );
    assert!(
        categories.contains(&DemoTipCategory::Updates),
        "Should have update tips"
    );
    assert!(
        categories.contains(&DemoTipCategory::Recovery),
        "Should have recovery tips"
    );
}

/// Test: All tip categories are available
/// Feature: demo_contact.feature @demo-content
#[test]
fn test_all_tip_categories_exist() {
    let all_categories = DemoTipCategory::all();

    assert_eq!(all_categories.len(), 6, "Should have 6 categories");
    assert!(all_categories.contains(&DemoTipCategory::GettingStarted));
    assert!(all_categories.contains(&DemoTipCategory::Privacy));
    assert!(all_categories.contains(&DemoTipCategory::Updates));
    assert!(all_categories.contains(&DemoTipCategory::Recovery));
    assert!(all_categories.contains(&DemoTipCategory::Contacts));
    assert!(all_categories.contains(&DemoTipCategory::Features));
}

/// Test: Tips rotate through all content
/// Feature: demo_contact.feature @demo-content
#[test]
fn test_demo_tips_rotate() {
    let mut state = DemoContactState::new_active();
    let tip_count = get_demo_tips().len();

    // Advance through all tips plus 2 extra to test wrap
    for _ in 0..tip_count + 2 {
        state.advance_to_next_tip();
    }

    // Index should wrap
    assert!(
        state.current_tip_index < tip_count,
        "Index should wrap around"
    );
}

// ============================================================
// Demo Contact Dismissal
// Scenario: Demo contact can be manually dismissed
// ============================================================

/// Test: Manual dismissal works
/// Feature: demo_contact.feature @demo-dismiss
#[test]
fn test_demo_contact_manual_dismiss() {
    let mut state = DemoContactState::new_active();
    assert!(state.is_active);

    state.dismiss();

    assert!(!state.is_active, "Should be inactive after dismiss");
    assert!(state.was_dismissed, "Should mark as dismissed");
    assert!(!state.auto_removed, "Should not be auto-removed");
}

/// Test: Auto-removal after first real exchange
/// Feature: demo_contact.feature @demo-dismiss
#[test]
fn test_demo_contact_auto_remove() {
    let mut state = DemoContactState::new_active();

    state.auto_remove();

    assert!(!state.is_active, "Should be inactive");
    assert!(!state.was_dismissed, "Should not be marked as dismissed");
    assert!(state.auto_removed, "Should be marked as auto-removed");
}

/// Test: Demo contact can be restored from settings
/// Feature: demo_contact.feature @demo-dismiss
#[test]
fn test_demo_contact_restore() {
    let mut state = DemoContactState::new_active();

    // Dismiss
    state.dismiss();
    assert!(!state.is_active);
    assert!(state.was_dismissed);

    // Restore
    state.restore();
    assert!(state.is_active, "Should be active after restore");
    assert!(!state.was_dismissed, "Dismissed flag should be cleared");
    assert!(!state.auto_removed, "Auto-removed flag should be cleared");
}

/// Test: Restore works after auto-remove too
/// Feature: demo_contact.feature @demo-dismiss
#[test]
fn test_demo_contact_restore_after_auto_remove() {
    let mut state = DemoContactState::new_active();

    state.auto_remove();
    assert!(state.auto_removed);

    state.restore();
    assert!(state.is_active);
    assert!(!state.auto_removed);
}

// ============================================================
// Demo Contact Privacy
// Scenario: Demo contact is local only
// ============================================================

/// Test: Demo contact card is self-contained
/// Feature: demo_contact.feature @demo-privacy
#[test]
fn test_demo_contact_local_only() {
    let state = DemoContactState::new_active();
    let tip = state.current_tip().unwrap();
    let card = generate_demo_contact_card(&tip);

    // Card contains all needed info locally
    assert!(!card.id.is_empty());
    assert!(!card.display_name.is_empty());
    assert!(!card.tip_title.is_empty());
    assert!(!card.tip_content.is_empty());
    assert!(!card.tip_category.is_empty());
}

// ============================================================
// Persistence
// Scenario: Demo contact state persists across app restarts
// ============================================================

/// Test: Demo state persists via serialization
/// Feature: demo_contact.feature @demo-persistence
#[test]
fn test_demo_state_persists() {
    let mut state = DemoContactState::new_active();

    // Make some changes
    state.advance_to_next_tip();
    state.advance_to_next_tip();

    // Serialize (simulate app quit)
    let json = state.to_json().expect("Should serialize");

    // Deserialize (simulate app restart)
    let restored = DemoContactState::from_json(&json).expect("Should deserialize");

    assert_eq!(restored.is_active, state.is_active);
    assert_eq!(restored.current_tip_index, state.current_tip_index);
    assert_eq!(restored.update_count, state.update_count);
}

/// Test: Dismissal persists across app restarts
/// Feature: demo_contact.feature @demo-persistence
#[test]
fn test_dismissal_persists() {
    let mut state = DemoContactState::new_active();
    state.dismiss();

    let json = state.to_json().unwrap();
    let restored = DemoContactState::from_json(&json).unwrap();

    assert!(!restored.is_active, "Should remain inactive");
    assert!(restored.was_dismissed, "Dismissal flag should persist");
}

/// Test: Update history persists
/// Feature: demo_contact.feature @demo-persistence
#[test]
fn test_update_history_persists() {
    let mut state = DemoContactState::new_active();

    // Generate some updates
    for _ in 0..3 {
        state.advance_to_next_tip();
    }

    let json = state.to_json().unwrap();
    let restored = DemoContactState::from_json(&json).unwrap();

    assert_eq!(restored.update_count, 3, "Update count should persist");
    assert!(
        !restored.shown_tip_ids.is_empty(),
        "Shown tips should persist"
    );
}

// ============================================================
// Edge Cases
// Scenario: Demo contact handles offline gracefully
// ============================================================

/// Test: Demo contact works offline (no network needed)
/// Feature: demo_contact.feature @demo-edge
#[test]
fn test_demo_works_offline() {
    // Demo contact is entirely local, no network calls
    let mut state = DemoContactState::new_active();

    // All operations are local
    let tip = state.current_tip();
    assert!(tip.is_some());

    let next = state.advance_to_next_tip();
    assert!(next.is_some());

    // Card generation is local
    let card = generate_demo_contact_card(&next.unwrap());
    assert!(card.is_demo);
}

/// Test: Update not due when inactive
/// Feature: demo_contact.feature @demo-edge
#[test]
fn test_update_not_due_when_inactive() {
    let mut state = DemoContactState::new_active();
    state.dismiss();

    assert!(!state.is_update_due(), "No updates when inactive");
}

// ============================================================
// Full Workflow
// ============================================================

/// Test: Full demo contact lifecycle
#[test]
fn test_demo_contact_full_lifecycle() {
    // Step 1: New user gets demo contact
    let mut state = DemoContactState::new_active();
    assert!(state.is_active);
    assert_eq!(state.current_tip_index, 0);

    // Step 2: User views initial tip
    let initial_tip = state.current_tip().unwrap();
    assert!(!initial_tip.title.is_empty());

    // Step 3: Time passes, updates come
    state.advance_to_next_tip();
    state.advance_to_next_tip();
    assert_eq!(state.update_count, 2);

    // Step 4: App restart
    let json = state.to_json().unwrap();
    let mut state = DemoContactState::from_json(&json).unwrap();
    assert_eq!(state.update_count, 2, "Should preserve state");

    // Step 5: User completes first real exchange
    state.auto_remove();
    assert!(!state.is_active);
    assert!(state.auto_removed);

    // Step 6: User can restore from settings
    state.restore();
    assert!(state.is_active);

    // Step 7: User manually dismisses
    state.dismiss();
    assert!(!state.is_active);
    assert!(state.was_dismissed);
}

/// Test: Demo contact card generation with different tips
#[test]
fn test_demo_card_generation_all_tips() {
    let tips = get_demo_tips();

    for tip in &tips {
        let card = generate_demo_contact_card(tip);

        assert!(card.is_demo);
        assert_eq!(card.id, DEMO_CONTACT_ID);
        assert_eq!(card.display_name, DEMO_CONTACT_NAME);
        assert_eq!(card.tip_title, tip.title);
        assert_eq!(card.tip_content, tip.content);
    }
}

/// Test: Tip shown tracking
#[test]
fn test_shown_tip_tracking() {
    let mut state = DemoContactState::new_active();

    // Initially has first tip shown
    assert!(!state.shown_tip_ids.is_empty());

    let initial_count = state.shown_tip_ids.len();

    // Advance through tips
    state.advance_to_next_tip();
    state.advance_to_next_tip();

    // Should have tracked more shown tips
    assert!(state.shown_tip_ids.len() >= initial_count);
}
