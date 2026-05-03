// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::*;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

// ── Test helpers ────────────────────────────────────────────────────

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("alice@example.com".into()),
        avatar_initials: "AL".into(),
        status: None,
        searchable_fields: vec![],
        actions: vec![],
        a11y: None,
    }
}

fn sample_fields() -> Vec<FieldDisplay> {
    vec![FieldDisplay {
        id: "f1".into(),
        label: "Email".into(),
        value: "alice@example.com".into(),
        field_type: "email".into(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }]
}

fn make_detail_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new())
}

// ── Personal note tests (Task 7) ─────────────────────────────────────

/// @scenario: contacts_management :: ContactDetail shows personal note
///
/// Verifies that a saved personal note is surfaced as an editable text
/// component on the ContactDetail screen when it is loaded.
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_personal_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Add a contact
    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([2u8; 32], card, shared_key);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // Save a personal note (plain UTF-8 bytes — app layer does not encrypt)
    let note_text = "Met at the Rust conference";
    vauchi
        .save_personal_notes(&bob_id, note_text.as_bytes())
        .unwrap();

    // Build the ContactDetail screen via AppEngine
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: bob_id.clone(),
    });

    assert_eq!(screen.screen_id, "contact_detail");

    // The screen must contain an EditableText component with id "personal_note"
    let note_component = screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == "personal_note")
    });
    assert!(
        note_component.is_some(),
        "ContactDetail screen must contain an EditableText component with id 'personal_note', \
         got components: {:?}",
        screen
            .components
            .iter()
            .map(|c| match c {
                Component::EditableText { id, .. } => format!("EditableText({id})"),
                Component::InfoPanel { id, .. } => format!("InfoPanel({id})"),
                Component::FieldList { id, .. } => format!("FieldList({id})"),
                _ => "other".into(),
            })
            .collect::<Vec<_>>()
    );

    // The component must contain the saved note text
    if let Some(Component::EditableText { value, .. }) = note_component {
        assert_eq!(
            value, note_text,
            "personal_note component must display the saved note text"
        );
    }
}

/// @scenario: contacts_management :: ContactDetail shows empty note when none saved
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_empty_note_when_no_note_saved() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let card = ContactCard::new("Carol");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([3u8; 32], card, shared_key);
    let carol_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // No note saved — personal_note component should be present but empty
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: carol_id,
    });

    let note_component = screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == "personal_note")
    });

    assert!(
        note_component.is_some(),
        "ContactDetail screen must always have a personal_note EditableText component"
    );
    if let Some(Component::EditableText { value, .. }) = note_component {
        assert_eq!(
            value, "",
            "personal_note must be empty when no note is saved"
        );
    }
}

/// @scenario: contacts_management :: Edit personal note persists via AppEngine
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_text_changed_saves_personal_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let card = ContactCard::new("Dave");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([4u8; 32], card, shared_key);
    let dave_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id.clone(),
    });

    // Simulate typing a note
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "personal_note".into(),
        value: "Colleague at Acme Corp".into(),
    });

    // Must return UpdateScreen (not an error or navigation)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged on personal_note should return UpdateScreen, got {result:?}"
    );

    // The note must be persisted — verify by reloading the screen
    let new_screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id,
    });
    let note_component = new_screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == "personal_note")
    });
    assert!(
        note_component.is_some(),
        "personal_note component must be present after save"
    );
    if let Some(Component::EditableText { value, .. }) = note_component {
        assert_eq!(
            value, "Colleague at Acme Corp",
            "saved note must be reflected when screen reloads"
        );
    }
}

// ── Field notes tests (Task 9) ────────────────────────────────────────

/// @scenario: contacts_management :: ContactDetail shows per-field private notes
///
/// Verifies that a saved field note is surfaced as an editable text
/// component (id `field_note:{field_id}`) on the ContactDetail screen.
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_field_notes() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    // Add a contact with a phone field so there is a real field_id to attach the note to
    let mut card = ContactCard::new("Bob");
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Phone,
        "Mobile",
        "+41 79 111 22 33",
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([10u8; 32], card, shared_key);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // Save a field note (plain UTF-8 bytes — same convention as personal_note)
    let note_text = "Work number, call after 9am";
    vauchi
        .save_contact_field_note(&bob_id, &field_id, note_text.as_bytes())
        .unwrap();

    // Build the ContactDetail screen via AppEngine
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: bob_id.clone(),
    });

    assert_eq!(screen.screen_id, "contact_detail");

    let expected_id = format!("field_note:{field_id}");

    // The screen must contain an EditableText component with id "field_note:{field_id}"
    let field_note_component = screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == &expected_id)
    });

    assert!(
        field_note_component.is_some(),
        "ContactDetail screen must contain an EditableText component with id '{expected_id}', \
         got components: {:?}",
        screen
            .components
            .iter()
            .map(|c| match c {
                Component::EditableText { id, .. } => format!("EditableText({id})"),
                Component::InfoPanel { id, .. } => format!("InfoPanel({id})"),
                Component::FieldList { id, .. } => format!("FieldList({id})"),
                _ => "other".into(),
            })
            .collect::<Vec<_>>()
    );

    // The component must contain the saved note text
    if let Some(Component::EditableText { value, .. }) = field_note_component {
        assert_eq!(
            value, note_text,
            "field_note component must display the saved note text"
        );
    }
}

/// @scenario: contacts_management :: ContactDetail shows empty field note when none saved
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_empty_field_note_when_none_saved() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let mut card = ContactCard::new("Carol");
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Email,
        "Work",
        "carol@example.com",
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([11u8; 32], card, shared_key);
    let carol_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // No field note saved
    let mut engine = AppEngine::new(vauchi);
    let screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: carol_id,
    });

    let expected_id = format!("field_note:{field_id}");
    let field_note_component = screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == &expected_id)
    });

    assert!(
        field_note_component.is_some(),
        "ContactDetail must always show a field_note EditableText for each shared field"
    );
    if let Some(Component::EditableText { value, .. }) = field_note_component {
        assert_eq!(value, "", "field_note must be empty when no note is saved");
    }
}

/// @scenario: contacts_management :: Editing a field note via TextChanged persists it
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_text_changed_saves_field_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let mut card = ContactCard::new("Dave");
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Phone,
        "Home",
        "+41 79 222 33 44",
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([12u8; 32], card, shared_key);
    let dave_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id.clone(),
    });

    let component_id = format!("field_note:{field_id}");

    // Simulate typing a field note
    let result = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.clone(),
        value: "Prefers text messages".into(),
    });

    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged on field_note should return UpdateScreen, got {result:?}"
    );

    // Reload the screen — saved note must appear
    let new_screen = engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id,
    });
    let field_note_component = new_screen.components.iter().find(|c| {
        matches!(c, Component::EditableText { id, ..
        } if id == &component_id)
    });

    assert!(
        field_note_component.is_some(),
        "field_note component must be present after save"
    );
    if let Some(Component::EditableText { value, .. }) = field_note_component {
        assert_eq!(
            value, "Prefers text messages",
            "saved field note must be reflected when screen reloads"
        );
    }
}

// ── ContactDetailEngine tests ───────────────────────────────────────

// @internal
#[test]
fn contact_detail_screen_id() {
    let engine = make_detail_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_detail");
}

// @internal
#[test]
fn contact_detail_shows_contact_name_as_title() {
    let engine = make_detail_engine();
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Alice");
}

// @internal
#[test]
fn contact_detail_edit_returns_edit_contact() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit".into(),
    });

    match result {
        ActionResult::EditContact { contact_id } => {
            assert_eq!(contact_id, "c1");
        }
        other => panic!("Expected EditContact, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_detail_back_completes() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}

// @internal
#[test]
fn contact_detail_unknown_action_returns_update_screen() {
    let mut engine = make_detail_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "nonexistent".into(),
    });

    match result {
        ActionResult::UpdateScreen(screen) => {
            assert_eq!(screen.screen_id, "contact_detail");
        }
        other => panic!("Expected UpdateScreen, got {:?}", other),
    }
}

// ── ContactNotFoundEngine tests ─────────────────────────────────────

// @internal
#[test]
fn contact_not_found_screen_id() {
    let engine = ContactNotFoundEngine::new("missing_id".into());
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "contact_not_found");
}

// @internal
#[test]
fn contact_not_found_shows_contact_id_in_error() {
    let engine = ContactNotFoundEngine::new("xyz_123".into());
    let screen = engine.current_screen();

    match &screen.components[0] {
        Component::InfoPanel { items, .. } => {
            let error_item = &items[0];
            assert!(
                error_item.detail.contains("xyz_123"),
                "Error detail should contain the contact id, got: {}",
                error_item.detail
            );
        }
        other => panic!("Expected InfoPanel, got {:?}", other),
    }
}

// @internal
#[test]
fn contact_not_found_back_completes() {
    let mut engine = ContactNotFoundEngine::new("missing".into());
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "back".into(),
    });

    assert_eq!(result, ActionResult::Complete);
}
