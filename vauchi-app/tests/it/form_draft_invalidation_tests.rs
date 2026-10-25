// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A background invalidation must not discard what the user has typed
//! into an open form. A sync that reports a delivery fires
//! `PresentationInvalidated`; on a Pixel 3a an Edit Display Name form
//! rebuilt by it saved the old name when Save came after the sync
//! (vauchi/private#799, seen on the 2026-10-24 rig run for #319).

use vauchi_app::ui::{AppEngine, AppScreen, FormDialogType, UserAction, WorkflowEngine};
use vauchi_core::contact_card::{ContactCard, ContactField, FieldType};
use vauchi_core::{Contact, Event, Identity, SymmetricKey, Vauchi};

fn engine_named(name: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity created");
    AppEngine::new(vauchi)
}

fn type_into(engine: &mut AppEngine, component_id: &str, value: &str) {
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.into(),
        value: value.into(),
    });
}

fn submit(engine: &mut AppEngine) {
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
}

fn invalidate(engine: &mut AppEngine) {
    let _ = engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation rebuild");
}

fn own_display_name(engine: &AppEngine) -> String {
    engine
        .vauchi()
        .own_card()
        .expect("own card loads")
        .expect("own card exists")
        .display_name()
        .to_string()
}

// @scenario: my_info.feature - Editing the display name
#[test]
fn a_name_typed_before_an_invalidation_is_the_name_saved() {
    let mut engine = engine_named("Rig64 Alice (E2E)");
    let _ = engine.navigate_to(AppScreen::Settings);
    let _ = engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Rig64 Alice (E2E)".into(),
        },
    });
    type_into(&mut engine, "display_name", "Rig64 Alice");

    invalidate(&mut engine);
    submit(&mut engine);

    assert_eq!(own_display_name(&engine), "Rig64 Alice");
}

// @scenario: contact_card_management.feature :: Edit an existing field value
#[test]
fn a_field_value_typed_before_an_invalidation_is_the_value_saved() {
    let mut engine = engine_named("Ana");
    let field = ContactField::new(
        FieldType::Email,
        "Work",
        "old@example.test",
        engine.vauchi().clock().unix_seconds(),
    );
    let field_id = field.id().to_string();
    engine
        .vauchi_mut()
        .add_own_field(field)
        .expect("field saves");
    let _ = engine.navigate_to(AppScreen::MyInfo);
    let _ = engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditField {
            field_id: field_id.clone(),
            field_label: "Work".into(),
            current_value: "old@example.test".into(),
            current_note: None,
        },
    });
    type_into(&mut engine, "field_value", "new@example.test");

    invalidate(&mut engine);
    submit(&mut engine);

    let card = engine.vauchi().own_card().unwrap().expect("own card");
    let saved = card
        .fields()
        .iter()
        .find(|f| f.id() == field_id)
        .expect("the field still exists");
    assert_eq!(saved.value(), "new@example.test");
}

// @scenario: contacts_management.feature :: Set and display custom nickname
#[test]
fn a_nickname_typed_before_an_invalidation_is_the_nickname_saved() {
    let mut engine = engine_named("Ada");
    let identity = Identity::create("Grace Hopper", 0);
    let contact = Contact::from_exchange(
        *identity.signing_public_key(),
        ContactCard::new("Grace Hopper"),
        SymmetricKey::generate(),
        0,
    );
    let contact_id = contact.id().to_string();
    engine.vauchi().add_contact(contact).expect("contact saves");
    let _ = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: contact_id.clone(),
    });
    let _ = engine.navigate_to(AppScreen::ContactEdit {
        contact_id: contact_id.clone(),
    });
    type_into(&mut engine, "display_name", "Amazing Grace");

    invalidate(&mut engine);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "contact_edit_actions".into(),
        item_id: "save".into(),
    });

    assert_eq!(
        engine
            .vauchi()
            .get_contact_nickname(&contact_id)
            .expect("nickname loads")
            .as_deref(),
        Some("Amazing Grace")
    );
}

// @scenario: duress_mode.feature :: Enable duress password (requires app password)
#[test]
fn a_password_typed_before_an_invalidation_is_the_password_set() {
    let mut engine = engine_named("Ana");
    let _ = engine.navigate_to(AppScreen::Settings);
    let _ = engine.navigate_to(AppScreen::ChangePassword);
    type_into(&mut engine, "new_password", "correct horse 591");
    type_into(&mut engine, "confirm_password", "correct horse 591");

    invalidate(&mut engine);
    submit(&mut engine);

    assert!(engine.vauchi().is_password_enabled().unwrap());
    assert!(
        engine
            .vauchi_mut()
            .authenticate("correct horse 591")
            .is_ok()
    );
    assert!(engine.vauchi_mut().authenticate("wrong horse 591").is_err());
}
