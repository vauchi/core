// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Blocking a contact from its screen (ADR-056, private#889): the question
//! before blocking with its optional wipe, the silence afterwards, the
//! blocked status with its Unblock, and the note when an emergency
//! contact is blocked.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ScreenModel, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::exchange::X3DHKeyPair;

fn engine_ready() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn add_exchanged_contact(engine: &AppEngine, name: &str, key_byte: u8) -> String {
    let contact = Contact::from_exchange(
        [key_byte; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    engine.vauchi().add_contact(contact).unwrap();
    id
}

/// A contact Alice can seal updates for: a shared field and a session.
fn add_reachable_contact(engine: &AppEngine, name: &str, key_byte: u8) -> String {
    engine
        .vauchi()
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let shared = SymmetricKey::generate();
    let contact = Contact::from_exchange([key_byte; 32], ContactCard::new(name), shared.clone(), 0);
    let id = contact.id().to_string();
    engine.vauchi().add_contact(contact).unwrap();
    engine
        .vauchi()
        .create_ratchet_as_initiator(&id, &shared, *X3DHKeyPair::generate().public_key())
        .unwrap();
    id
}

fn open_detail(engine: &mut AppEngine, contact_id: &str) -> ScreenModel {
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.into(),
    });
    engine.current_screen()
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn toggle(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ItemToggled {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn action_ids(screen: &ScreenModel) -> Vec<String> {
    screen
        .contextual_actions
        .iter()
        .map(|a| a.id.clone())
        .collect()
}

fn screen_of(result: ActionResult) -> ScreenModel {
    match result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => s,
        other => panic!("expected a screen, got {other:?}"),
    }
}

fn toast_of(result: ActionResult) -> String {
    match result {
        ActionResult::ShowToast { message, .. } => message,
        other => panic!("expected a toast, got {other:?}"),
    }
}

fn inline_confirm_ids(screen: &ScreenModel, id: &str) -> Option<(String, String)> {
    screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm {
            id: component_id,
            confirm_action_id,
            cancel_action_id,
            ..
        } if component_id == id => Some((confirm_action_id.clone(), cancel_action_id.clone())),
        _ => None,
    })
}

fn wipe_toggle_selected(screen: &ScreenModel) -> Option<bool> {
    screen.components.iter().find_map(|c| match c {
        Component::ToggleList { id, items, .. } if id == "block_options" => items
            .iter()
            .find(|item| item.id == "wipe_their_copy")
            .map(|item| item.selected),
        _ => None,
    })
}

fn blocked_status_label(screen: &ScreenModel) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator {
            id, status_label, ..
        } if id == "blocked_status" => Some(status_label.clone()),
        _ => None,
    })
}

fn pending_for(engine: &AppEngine, contact_id: &str) -> usize {
    engine
        .vauchi()
        .storage()
        .pending()
        .count_pending_updates(contact_id)
        .unwrap()
}

// @scenario: visibility_control :: Block contact removes all their visibility
#[test]
fn blocking_a_contact_is_reachable_and_silent() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 7);
    let screen = open_detail(&mut engine, &bob);
    assert!(
        action_ids(&screen).contains(&"block_contact".to_string()),
        "an exchanged contact can be blocked, got {:?}",
        action_ids(&screen)
    );
    assert_eq!(blocked_status_label(&screen), None, "not blocked yet");

    let asked = screen_of(press(&mut engine, "block_contact"));
    assert_eq!(
        inline_confirm_ids(&asked, "block_contact"),
        Some((
            "confirm_block_contact".to_string(),
            "cancel_block_contact".to_string()
        )),
        "blocking asks first"
    );
    assert_eq!(
        wipe_toggle_selected(&asked),
        Some(false),
        "the wipe is offered and off by default"
    );

    let toast = toast_of(press(&mut engine, "confirm_block_contact"));

    assert_eq!(toast, "Blocked Bob.");
    assert!(
        engine
            .vauchi()
            .get_contact(&bob)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
    assert_eq!(
        pending_for(&engine, &bob),
        0,
        "a silent block sends nothing"
    );
    let after = engine.current_screen();
    assert_eq!(blocked_status_label(&after), Some("Blocked".to_string()));
    let ids = action_ids(&after);
    assert!(ids.contains(&"unblock_contact".to_string()), "got {ids:?}");
    assert!(!ids.contains(&"block_contact".to_string()), "got {ids:?}");
    assert_eq!(
        inline_confirm_ids(&after, "block_contact"),
        None,
        "the question is gone"
    );
}

// @internal
#[test]
fn the_block_question_can_be_cancelled() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 7);
    open_detail(&mut engine, &bob);
    screen_of(press(&mut engine, "block_contact"));

    let screen = screen_of(press(&mut engine, "cancel_block_contact"));

    assert_eq!(inline_confirm_ids(&screen, "block_contact"), None);
    assert_eq!(
        wipe_toggle_selected(&screen),
        None,
        "the wipe toggle leaves with the question"
    );
    assert!(
        !engine
            .vauchi()
            .get_contact(&bob)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
}

// @scenario: visibility_control :: Blocking offers a final wipe of my info
#[test]
fn choosing_the_wipe_sends_one_last_update_and_says_so() {
    let mut engine = engine_ready();
    let bob = add_reachable_contact(&engine, "Bob", 7);
    open_detail(&mut engine, &bob);
    screen_of(press(&mut engine, "block_contact"));
    let armed = screen_of(toggle(&mut engine, "block_options", "wipe_their_copy"));
    assert_eq!(wipe_toggle_selected(&armed), Some(true));

    let toast = toast_of(press(&mut engine, "confirm_block_contact"));

    assert_eq!(
        toast,
        "Blocked Bob. Your info will be removed from their device."
    );
    assert_eq!(pending_for(&engine, &bob), 1, "exactly one final update");
    assert!(
        engine
            .vauchi()
            .get_contact(&bob)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
}

// @scenario: visibility_control :: Blocking offers a final wipe of my info
#[test]
fn choosing_the_wipe_for_a_contact_without_a_session_says_the_info_could_not_be_removed() {
    let mut engine = engine_ready();
    engine
        .vauchi()
        .add_own_field(ContactField::new(FieldType::Email, "work", "a@co.com", 0))
        .unwrap();
    let bob = add_exchanged_contact(&engine, "Bob", 7);
    open_detail(&mut engine, &bob);
    screen_of(press(&mut engine, "block_contact"));
    screen_of(toggle(&mut engine, "block_options", "wipe_their_copy"));

    let toast = toast_of(press(&mut engine, "confirm_block_contact"));

    assert_eq!(
        toast,
        "Blocked Bob. Your info could not be removed from their device."
    );
    assert_eq!(pending_for(&engine, &bob), 0);
    assert!(
        engine
            .vauchi()
            .get_contact(&bob)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
}

// @scenario: emergency_broadcast :: Blocked contact in trusted list
#[test]
fn blocking_an_emergency_contact_says_they_were_removed_from_the_list() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 7);
    let carol = add_exchanged_contact(&engine, "Carol", 8);
    engine
        .vauchi_mut()
        .configure_emergency_broadcast(vec![bob.clone(), carol.clone()], "Help".into(), false)
        .unwrap();
    open_detail(&mut engine, &bob);
    screen_of(press(&mut engine, "block_contact"));

    let toast = toast_of(press(&mut engine, "confirm_block_contact"));

    assert_eq!(
        toast,
        "Blocked Bob. Bob was also removed from your emergency contacts."
    );
    assert_eq!(
        engine
            .vauchi()
            .load_emergency_config()
            .unwrap()
            .unwrap()
            .trusted_contact_ids,
        vec![carol]
    );
}

// @scenario: contacts_management :: Unblock a contact
#[test]
fn a_blocked_contact_shows_it_and_can_be_unblocked_from_its_screen() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 7);
    engine.vauchi().block_contact(&bob).unwrap();

    let screen = open_detail(&mut engine, &bob);
    assert_eq!(blocked_status_label(&screen), Some("Blocked".to_string()));
    assert!(action_ids(&screen).contains(&"unblock_contact".to_string()));

    let toast = toast_of(press(&mut engine, "unblock_contact"));

    assert_eq!(toast, "Unblocked Bob");
    assert!(
        !engine
            .vauchi()
            .get_contact(&bob)
            .unwrap()
            .unwrap()
            .is_blocked()
    );
    let after = engine.current_screen();
    assert_eq!(blocked_status_label(&after), None);
    assert!(action_ids(&after).contains(&"block_contact".to_string()));
}

// @internal
#[test]
fn an_imported_contact_cannot_be_blocked() {
    let mut engine = engine_ready();
    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n";
    engine.vauchi().import_contacts_from_vcf(vcf).unwrap();
    let bob = engine.vauchi().list_contacts().unwrap()[0].id().to_string();

    let screen = open_detail(&mut engine, &bob);

    let ids = action_ids(&screen);
    assert!(!ids.contains(&"block_contact".to_string()), "got {ids:?}");
    assert!(!ids.contains(&"unblock_contact".to_string()), "got {ids:?}");
}
