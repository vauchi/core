// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Integration tests for the AppEngine merge flow wiring.
//!
//! Covers the full DuplicateDetection → ContactMerge → completion path:
//! - Pressing "merge" on ContactDuplicates navigates to ContactMerge
//! - ContactMerge preview carries correct names
//! - Pressing "confirm" calls merge_contacts and shows a success toast
//! - Caches are invalidated after merge
//! - Missing pending_merge state returns a safe fallback

use vauchi_app::ui::ActionResult;
use vauchi_app::ui::AppEngine;
use vauchi_app::ui::AppScreen;
use vauchi_app::ui::Component;
use vauchi_app::ui::UserAction;
use vauchi_app::ui::WorkflowEngine;
use vauchi_app::ui::*;
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

fn new_vauchi_with_identity() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Test User").unwrap();
    vauchi
}

fn add_imported(vauchi: &Vauchi, name: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(1);
    let card = ContactCard::new(name);
    let contact_id = format!("contact-{}-{name}", COUNTER.fetch_add(1, Ordering::Relaxed));
    let contact = Contact::from_import(contact_id, card, ImportSource::VcardFile, None, 0);
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

// @scenario: contacts_management :: Merge duplicate contacts (AppEngine wiring)
#[test]
fn pressing_merge_on_duplicates_navigates_to_contact_merge() {
    let vauchi = new_vauchi_with_identity();
    add_imported(&vauchi, "Alice Smith");
    add_imported(&vauchi, "Alice Smith");

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);

    let screen = engine.current_screen();
    assert_eq!(
        screen.screen_id, "duplicate_detection",
        "should be on duplicate_detection screen before pressing merge"
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });

    match result {
        ActionResult::NavigateTo(ref screen_model) => {
            assert_eq!(
                screen_model.screen_id, "contact_merge",
                "pressing merge must navigate to contact_merge, got: {}",
                screen_model.screen_id
            );
        }
        other => panic!(
            "pressing merge on ContactDuplicates must return NavigateTo(contact_merge), got: {other:?}"
        ),
    }
}

// @scenario: contacts_management :: Merge duplicate contacts (preview content)
#[test]
fn contact_merge_preview_shows_contact_names() {
    let vauchi = new_vauchi_with_identity();
    add_imported(&vauchi, "Alice Smith");
    add_imported(&vauchi, "Alice Smith");

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });

    let screen = engine.current_screen();
    assert_eq!(
        screen.screen_id, "contact_merge",
        "current screen must be contact_merge after pressing merge"
    );

    let has_name = screen.components.iter().any(|c| match c {
        Component::Text { content, .. } => content.contains("Alice Smith"),
        Component::InfoPanel { title, .. } => title.contains("Alice Smith"),
        _ => false,
    });
    assert!(
        has_name,
        "merge preview must reference the contact names; components: {:?}",
        screen
            .components
            .iter()
            .map(|c| format!("{c:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

// @scenario: contacts_management :: Merge duplicate contacts (confirm executes merge)
#[test]
fn confirm_merge_calls_merge_and_shows_success_toast() {
    let vauchi = new_vauchi_with_identity();
    let id1 = add_imported(&vauchi, "Alice Smith");
    let id2 = add_imported(&vauchi, "Alice Smith");

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });

    match &result {
        ActionResult::ShowToast { message, .. } => {
            assert!(
                message.to_lowercase().contains("merged"),
                "toast message must mention 'merged', got: {message}"
            );
        }
        other => panic!("confirming merge must return ShowToast, got: {other:?}"),
    }

    // Exactly one contact must survive the merge
    let remaining = engine.vauchi().list_contacts().unwrap();
    assert_eq!(
        remaining.len(),
        1,
        "after merge exactly one contact must remain, got {}",
        remaining.len()
    );

    let surviving_id = remaining[0].id().to_string();
    assert!(
        surviving_id == id1 || surviving_id == id2,
        "surviving contact must be one of the original pair, got: {surviving_id}"
    );
}

// @scenario: contacts_management :: Merge duplicate contacts (no-pairs fallback)
#[test]
fn merge_action_with_no_duplicates_does_not_navigate_to_merge_screen() {
    let vauchi = new_vauchi_with_identity();
    // One contact only — no duplicate pairs possible
    add_imported(&vauchi, "Solo Contact");

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDuplicates);

    // The "merge" button is disabled when there are no pairs, but if the action
    // fires anyway it must not panic or navigate to the merge screen.
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "merge".into(),
    });

    assert!(
        !matches!(result, ActionResult::NavigateTo(ref s) if s.screen_id == "contact_merge"),
        "merge with no duplicate pairs must not navigate to contact_merge, got: {result:?}"
    );
}

// ── Test helpers ────────────────────────────────────────────────────

fn sample_preview() -> MergePreview {
    MergePreview {
        primary_name: "Alice".into(),
        primary_fields: vec!["Email: alice@example.com".into(), "Phone: +1234".into()],
        secondary_name: "Alicia".into(),
        secondary_fields: vec!["Email: alicia@work.com".into()],
    }
}

fn make_engine() -> ContactMergeEngine {
    ContactMergeEngine::new(sample_preview())
}

// ── Tests ───────────────────────────────────────────────────────────

// @internal
#[test]
fn contact_merge_screen_id() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_merge");
}

// @internal
#[test]
fn contact_merge_title_is_merge_contacts() {
    let engine = make_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Merge Contacts");
}

// @internal
#[test]
fn contact_merge_shows_both_contact_names() {
    let engine = make_engine();
    let screen = engine.current_screen();

    // The first Text component contains both names
    match &screen.components[0] {
        Component::Text { content, .. } => {
            assert!(
                content.contains("Alice"),
                "Expected content to contain 'Alice', got: {}",
                content
            );
            assert!(
                content.contains("Alicia"),
                "Expected content to contain 'Alicia', got: {}",
                content
            );
        }
        other => panic!("Expected Text component, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_merge_confirm_completes() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn contact_merge_cancel_stays_on_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_merge");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_merge_unknown_action_returns_update_screen() {
    let mut engine = make_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_merge");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}
