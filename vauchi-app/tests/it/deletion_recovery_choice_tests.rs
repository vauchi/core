// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Scheduling an identity deletion asks whether recovery stays possible:
//! keep the relay's list of trusted contacts for up to 365 days, or erase
//! everything. Someone with no trusted contacts has nothing to keep and is
//! not asked (#711; owner decision 2026-10-20).

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::DeletionManager;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::storage::DeletionState;
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn privacy(with_trusted_contact: bool) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    if with_trusted_contact {
        let friend = Identity::create("Amira", 0);
        let contact = Contact::from_exchange(
            *friend.signing_public_key(),
            ContactCard::new("Amira"),
            SymmetricKey::generate(),
            0,
        );
        let id = contact.id().to_string();
        vauchi.add_contact(contact).unwrap();
        // Recovery trust needs a Verified or High trust level (ADR-034).
        vauchi.verify_contact_fingerprint(&id).unwrap();
        vauchi.toggle_recovery_trust(&id).unwrap();
    }
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Privacy);
    engine
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn confirm_delete(engine: &mut AppEngine) -> ActionResult {
    let _ = press(engine, "delete");
    press(engine, "confirm_delete")
}

fn state(engine: &AppEngine) -> DeletionState {
    DeletionManager::new(engine.vauchi().storage())
        .deletion_state()
        .unwrap()
}

fn erases_guardian_set(engine: &AppEngine) -> bool {
    DeletionManager::new(engine.vauchi().storage())
        .scheduled_erases_guardian_set()
        .unwrap()
}

// @scenario: privacy_compliance :: Deleting my identity asks whether recovery stays possible
#[test]
fn with_trusted_contacts_the_user_chooses_before_anything_is_scheduled() {
    let mut engine = privacy(true);

    let _ = confirm_delete(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "delete_recovery_choice");
    let actions: Vec<_> = screen
        .contextual_actions
        .iter()
        .map(|action| action.id.as_str())
        .collect();
    assert_eq!(actions, ["keep_recovery", "erase_everything", "cancel"]);
    assert_eq!(state(&engine), DeletionState::None);
}

// @scenario: privacy_compliance :: Deleting my identity asks whether recovery stays possible
#[test]
fn erasing_everything_schedules_a_deletion_that_erases_the_guardian_set() {
    let mut engine = privacy(true);
    let _ = confirm_delete(&mut engine);

    let _ = press(&mut engine, "erase_everything");

    assert!(matches!(state(&engine), DeletionState::Scheduled { .. }));
    assert!(erases_guardian_set(&engine));
}

// @scenario: privacy_compliance :: Deleting my identity asks whether recovery stays possible
#[test]
fn keeping_recovery_schedules_a_deletion_that_keeps_the_guardian_set() {
    let mut engine = privacy(true);
    let _ = confirm_delete(&mut engine);

    let _ = press(&mut engine, "keep_recovery");

    assert!(matches!(state(&engine), DeletionState::Scheduled { .. }));
    assert!(!erases_guardian_set(&engine));
}

// @scenario: privacy_compliance :: Deleting my identity asks whether recovery stays possible
#[test]
fn cancelling_the_choice_schedules_nothing() {
    let mut engine = privacy(true);
    let _ = confirm_delete(&mut engine);

    let _ = press(&mut engine, "cancel");

    assert_eq!(engine.current_screen().screen_id, "privacy_settings");
    assert_eq!(state(&engine), DeletionState::None);
}

// @scenario: privacy_compliance :: With nobody trusted for recovery, deleting erases everything
#[test]
fn without_trusted_contacts_there_is_nothing_to_keep_and_no_question() {
    let mut engine = privacy(false);

    let _ = confirm_delete(&mut engine);

    assert!(matches!(state(&engine), DeletionState::Scheduled { .. }));
    assert!(erases_guardian_set(&engine));
}
