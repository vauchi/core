// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Edit Contact is one form (#451, design pass #419 item 7; owner
//! decisions 2026-09-30): the name you see the contact by and your
//! personal note about them, a "Use their name" reset, Save as a body
//! button, and Back with unsaved changes asks before discarding. The
//! retired visibility and preview steps edited nothing that was saved.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, ContactEditEngine, EditableContact, ScreenModel,
    UserAction, WorkflowEngine,
};
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{Contact, Identity, SymmetricKey, Vauchi};

fn grace(display_name: &str, note: &str) -> ContactEditEngine {
    ContactEditEngine::new(EditableContact {
        card_name: "Grace Hopper".into(),
        display_name: display_name.into(),
        personal_note: note.into(),
    })
}

fn input(screen: &ScreenModel, id: &str) -> Option<(String, String, Option<String>)> {
    screen.components.iter().find_map(|c| match c {
        Component::TextInput {
            id: input_id,
            label,
            value,
            validation_error,
            ..
        } if input_id == id => Some((label.clone(), value.clone(), validation_error.clone())),
        _ => None,
    })
}

fn button(screen: &ScreenModel, list: &str, id: &str) -> Option<String> {
    screen.components.iter().find_map(|c| match c {
        Component::ButtonList { id: list_id, items } if list_id == list => {
            items.iter().find(|i| i.id == id).map(|i| i.label.clone())
        }
        _ => None,
    })
}

fn discard_prompt(screen: &ScreenModel) -> Option<(String, String, String)> {
    screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm {
            id,
            warning,
            confirm_text,
            cancel_text,
            ..
        } if id == "discard_changes" => {
            Some((warning.clone(), confirm_text.clone(), cancel_text.clone()))
        }
        _ => None,
    })
}

fn type_into(engine: &mut impl WorkflowEngine, id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: id.into(),
        value: value.into(),
    });
}

fn select(engine: &mut impl WorkflowEngine, list: &str, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: list.into(),
        item_id: id.into(),
    })
}

fn press(engine: &mut impl WorkflowEngine, id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: id.into(),
    })
}

// @internal
#[test]
fn one_form_with_name_note_and_save() {
    let screen = grace("Grace Hopper", "Met at the conference").current_screen();

    assert_eq!(screen.title, "Edit Contact");
    assert_eq!(
        input(&screen, "display_name"),
        Some(("Name".into(), "Grace Hopper".into(), None))
    );
    assert_eq!(
        input(&screen, "personal_note"),
        Some(("Personal note".into(), "Met at the conference".into(), None))
    );
    assert!(screen.components.iter().any(|c| matches!(
        c,
        Component::Text { content, .. }
            if content == "Only you see this name. Their card keeps the name they chose."
    )));
    assert_eq!(
        button(&screen, "contact_edit_actions", "save").as_deref(),
        Some("Save")
    );
    assert!(screen.contextual_actions.is_empty());
    assert!(
        screen.progress.is_none(),
        "no steps: one form replaces the three-step flow"
    );
}

// @internal
#[test]
fn use_their_name_offers_the_name_they_chose() {
    let mut engine = grace("Amazing Grace", "");
    assert_eq!(
        button(&engine.current_screen(), "name_actions", "use_card_name").as_deref(),
        Some("Use their name (Grace Hopper)")
    );

    let _ = select(&mut engine, "name_actions", "use_card_name");
    let screen = engine.current_screen();
    assert_eq!(input(&screen, "display_name").unwrap().1, "Grace Hopper");
    assert_eq!(button(&screen, "name_actions", "use_card_name"), None);

    let own_name = grace("Grace Hopper", "").current_screen();
    assert_eq!(button(&own_name, "name_actions", "use_card_name"), None);
}

// @internal
#[test]
fn save_needs_a_name() {
    let mut engine = grace("Grace Hopper", "");
    type_into(&mut engine, "display_name", "   ");
    assert_eq!(
        select(&mut engine, "contact_edit_actions", "save"),
        ActionResult::ValidationError {
            component_id: "display_name".into(),
            message: "Name is required".into(),
        }
    );
}

// @internal
#[test]
fn back_with_unsaved_changes_asks_first() {
    let mut untouched = grace("Grace Hopper", "");
    assert!(!untouched.navigate_back_within(), "nothing to lose: leave");

    let mut engine = grace("Grace Hopper", "");
    type_into(&mut engine, "personal_note", "Owes me a book");
    assert!(
        engine.navigate_back_within(),
        "unsaved changes: stay and ask"
    );
    assert_eq!(
        discard_prompt(&engine.current_screen()),
        Some((
            "Discard your changes?".into(),
            "Discard".into(),
            "Keep editing".into()
        ))
    );

    let _ = press(&mut engine, "keep_editing");
    let screen = engine.current_screen();
    assert_eq!(discard_prompt(&screen), None);
    assert_eq!(input(&screen, "personal_note").unwrap().1, "Owes me a book");

    assert!(engine.navigate_back_within(), "asks again");
    assert_eq!(
        press(&mut engine, "discard_changes"),
        ActionResult::Complete
    );
}

fn contact(vauchi: &Vauchi, name: &str) -> String {
    let identity = Identity::create(name, 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    let id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();
    id
}

fn note(engine: &AppEngine, contact_id: &str) -> Option<String> {
    engine
        .vauchi()
        .load_personal_notes(contact_id)
        .unwrap()
        .map(|bytes| String::from_utf8(bytes).unwrap())
}

fn open_edit(engine: &mut AppEngine, contact_id: &str) {
    let _ = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.into(),
    });
    let _ = engine.navigate_to(AppScreen::ContactEdit {
        contact_id: contact_id.into(),
    });
}

// @internal
#[test]
fn save_stores_the_nickname_and_note_and_use_their_name_clears_it() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let id = contact(&vauchi, "Grace Hopper");
    let mut engine = AppEngine::new(vauchi);

    open_edit(&mut engine, &id);
    type_into(&mut engine, "display_name", "Amazing Grace");
    type_into(&mut engine, "personal_note", "Met at the conference");
    let _ = select(&mut engine, "contact_edit_actions", "save");

    assert_eq!(engine.current_screen().screen_id, "contact_detail");
    assert_eq!(
        engine
            .vauchi()
            .get_contact_nickname(&id)
            .unwrap()
            .as_deref(),
        Some("Amazing Grace")
    );
    assert_eq!(note(&engine, &id).as_deref(), Some("Met at the conference"));

    open_edit(&mut engine, &id);
    assert_eq!(
        input(&engine.current_screen(), "personal_note").unwrap().1,
        "Met at the conference",
        "the form shows the note the contact screen shows"
    );
    let _ = select(&mut engine, "name_actions", "use_card_name");
    let _ = select(&mut engine, "contact_edit_actions", "save");
    assert_eq!(engine.vauchi().get_contact_nickname(&id).unwrap(), None);
    assert_eq!(note(&engine, &id).as_deref(), Some("Met at the conference"));
}

// @internal
#[test]
fn discarding_leaves_storage_untouched() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let id = contact(&vauchi, "Grace Hopper");
    let mut engine = AppEngine::new(vauchi);

    open_edit(&mut engine, &id);
    type_into(&mut engine, "display_name", "Amazing Grace");
    type_into(&mut engine, "personal_note", "Owes me a book");
    let asked = engine.navigate_back();
    assert_eq!(asked.screen_id, "contact_edit");
    assert!(discard_prompt(&asked).is_some());

    let _ = press(&mut engine, "discard_changes");
    assert_eq!(engine.current_screen().screen_id, "contact_detail");
    assert_eq!(engine.vauchi().get_contact_nickname(&id).unwrap(), None);
    assert_eq!(note(&engine, &id), None);
}
