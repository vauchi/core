// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tests for contact delete InlineConfirm behavior (SP-19 compliance).
//!
//! Imported contacts must use InlineConfirm → hard delete (irrevocable).
//! Exchanged contacts keep ShowToast + undo for archive (unchanged).

use vauchi_app::ui::AccessibilityRole;
use vauchi_app::ui::ActionResult;
use vauchi_app::ui::Component;
use vauchi_app::ui::ContactDetailEngine;
use vauchi_app::ui::ContactTag;
use vauchi_app::ui::Field;
use vauchi_app::ui::Item;
use vauchi_app::ui::UiFieldVisibility;
use vauchi_app::ui::UserAction;
use vauchi_app::ui::WorkflowEngine;
use vauchi_app::ui::*;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

fn sample_contact() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("+41 79 123 45 67".into()),
        initials: "A".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

fn sample_fields() -> Vec<Field> {
    vec![Field {
        id: "f1".into(),
        field_type: "Phone".into(),
        label: "Mobile".into(),
        value: "+41 79 123 45 67".into(),
        icon: "phone".into(),
        visibility_label: String::new(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }]
}

fn imported_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(true)
}

fn exchanged_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(sample_contact(), sample_fields(), String::new()).with_imported(false)
}

// @scenario: contact_detail.feature - Delete imported contact shows InlineConfirm
// @internal
#[test]
fn delete_contact_shows_inline_confirm() {
    let mut engine = imported_engine();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Must update screen (not ShowToast)
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "delete_contact on imported contact must return UpdateScreen, got: {result:?}"
    );

    // Screen must contain InlineConfirm component
    let screen = engine.current_screen();
    let has_inline_confirm = screen.components.iter().any(|c| {
        matches!(c, Component::InlineConfirm { id, destructive, .. }
            if id == "delete_contact" && *destructive)
    });
    assert!(
        has_inline_confirm,
        "Screen must contain a destructive InlineConfirm with id 'delete_contact'"
    );
}

// @scenario: contact_detail.feature - Delete imported contact shows InlineConfirm
// @internal
#[test]
fn inline_confirm_carries_core_owned_action_ids() {
    let mut engine = imported_engine();
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    let component = engine
        .current_screen()
        .components
        .into_iter()
        .find(|component| matches!(component, Component::InlineConfirm { .. }))
        .expect("delete flow must emit InlineConfirm");
    let Component::InlineConfirm {
        confirm_action_id,
        cancel_action_id,
        ..
    } = component
    else {
        unreachable!()
    };
    assert_eq!(confirm_action_id, "confirm_delete_contact");
    assert_eq!(cancel_action_id, "cancel_delete_contact");
}

// @scenario: contact_detail.feature - Confirm delete completes engine
// @internal
#[test]
fn confirm_delete_contact_completes() {
    let mut engine = imported_engine();

    // First trigger the InlineConfirm
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Then confirm
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_delete_contact".into(),
    });

    assert_eq!(
        result,
        ActionResult::Complete,
        "confirm_delete_contact must return Complete"
    );
}

// @scenario: contact_detail.feature - Cancel delete removes InlineConfirm
// @internal
#[test]
fn cancel_delete_contact_removes_inline_confirm() {
    let mut engine = imported_engine();

    // Trigger InlineConfirm
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    // Cancel
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_delete_contact".into(),
    });

    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "cancel must return UpdateScreen"
    );

    // InlineConfirm must be gone
    let screen = engine.current_screen();
    let has_inline_confirm = screen
        .components
        .iter()
        .any(|c| matches!(c, Component::InlineConfirm { .. }));
    assert!(
        !has_inline_confirm,
        "InlineConfirm must be removed after cancel"
    );
}

// @scenario: accessibility :: ContactDetail personal note EditableText has populated a11y
//
// Verifies that the personal_note EditableText component carries a meaningful
// accessibility label and TextField role so screen readers can announce it.
// @internal
#[test]
fn contact_detail_personal_note_has_a11y() {
    let engine = exchanged_engine();
    let screen = engine.current_screen();

    let personal_note = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::EditableText { id, .. } if id == "personal_note"));

    let personal_note =
        personal_note.expect("Screen must contain a personal_note EditableText component");

    match personal_note {
        Component::EditableText { a11y, .. } => {
            let a11y = a11y
                .as_ref()
                .expect("personal_note EditableText must have a11y populated");
            assert_eq!(
                a11y.label.as_deref(),
                Some("Personal note, editable"),
                "a11y label must identify the field as editable"
            );
            assert_eq!(
                a11y.hint.as_deref(),
                Some("Double tap to edit"),
                "a11y hint must describe how to activate editing"
            );
            assert_eq!(
                a11y.role,
                Some(AccessibilityRole::TextField),
                "EditableText role must be TextField"
            );
        }
        other => panic!("expected EditableText, got {:?}", other),
    }
}

// @scenario: contact_detail.feature - Edit private note
// @internal
#[test]
fn editable_text_carries_core_owned_action_ids() {
    let engine = exchanged_engine();
    let component = engine
        .current_screen()
        .components
        .into_iter()
        .find(|component| {
            matches!(component, Component::EditableText { id, .. } if id == "personal_note")
        })
        .expect("contact detail must emit personal-note EditableText");
    let Component::EditableText {
        edit_action_id,
        save_action_id,
        cancel_action_id,
        ..
    } = component
    else {
        unreachable!()
    };
    assert_eq!(edit_action_id, "edit_personal_note");
    assert_eq!(save_action_id, "save_personal_note");
    assert_eq!(cancel_action_id, "cancel_personal_note");
}

// @scenario: contact_detail.feature - Edit private note
// @internal
#[test]
fn editable_text_mode_is_driven_by_core_actions() {
    let mut engine = exchanged_engine();
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit_personal_note".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("edit action must update the core screen")
    };
    assert!(screen.components.iter().any(|component| matches!(
        component,
        Component::EditableText { id, editing: true, .. } if id == "personal_note"
    )));

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "personal_note".into(),
        value: "draft that will be cancelled".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("text change must update the core-owned draft")
    };
    assert!(screen.components.iter().any(|component| matches!(
        component,
        Component::EditableText { id, value, editing: true, .. }
            if id == "personal_note" && value == "draft that will be cancelled"
    )));

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_personal_note".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("cancel action must update the core screen")
    };
    assert!(screen.components.iter().any(|component| matches!(
        component,
        Component::EditableText { id, value, editing: false, .. }
            if id == "personal_note" && value.is_empty()
    )));

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit_personal_note".into(),
    });
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "personal_note".into(),
        value: "saved by core".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save_personal_note".into(),
    });
    let ActionResult::UpdateScreen(screen) = result else {
        panic!("save action must update the core screen")
    };
    assert!(screen.components.iter().any(|component| matches!(
        component,
        Component::EditableText { id, value, editing: false, .. }
            if id == "personal_note" && value == "saved by core"
    )));
}

// @scenario: accessibility :: ContactDetail delete InlineConfirm has populated a11y
//
// Verifies that the delete confirmation InlineConfirm carries an Alert role
// and descriptive label so screen readers announce it as a destructive action.
// @internal
#[test]
fn contact_detail_delete_inline_confirm_has_a11y() {
    let mut engine = imported_engine();

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "delete_contact".into(),
    });

    let screen = engine.current_screen();
    let confirm = screen
        .components
        .iter()
        .find(|c| matches!(c, Component::InlineConfirm { id, .. } if id == "delete_contact"));

    let confirm = confirm.expect("Screen must contain delete_contact InlineConfirm");

    match confirm {
        Component::InlineConfirm { a11y, .. } => {
            let a11y = a11y
                .as_ref()
                .expect("delete InlineConfirm must have a11y populated");
            assert_eq!(
                a11y.label.as_deref(),
                Some("Confirm contact deletion"),
                "a11y label must identify the confirmation"
            );
            assert_eq!(
                a11y.role,
                Some(AccessibilityRole::Alert),
                "InlineConfirm for destructive action must have Alert role"
            );
        }
        other => panic!("expected InlineConfirm, got {:?}", other),
    }
}

// @scenario: contact_detail.feature - Archive exchanged contact still uses ShowToast
// @internal
#[test]
fn archive_contact_still_uses_show_toast() {
    let mut engine = exchanged_engine();

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "archive_contact".into(),
    });

    assert_eq!(
        result,
        ActionResult::ShowToast {
            message: "Contact archived".into(),
            undo_action_id: Some("undo_archive_contact:c1".into()),
            undo_label: Some("Undo".into()),
        },
        "archive_contact must still return ShowToast with undo"
    );
}

// ── Phase 4a: contact tags (owner-private annotation vocabulary) ───────────
//
// Engine-level rendering only. Add/remove persistence flows through the
// AppEngine intercept (it needs `Vauchi`) and is covered separately.

/// Find the items of a flat `ActionList` with the given id, if present.
fn action_list_items<'a>(
    screen: &'a vauchi_app::ui::ScreenModel,
    id: &str,
) -> Option<&'a Vec<vauchi_app::ui::ActionListItem>> {
    screen.components.iter().find_map(|c| match c {
        Component::ActionList { id: cid, items } if cid == id => Some(items),
        _ => None,
    })
}

fn tagged_engine() -> ContactDetailEngine {
    exchanged_engine().with_tags(vec![
        ContactTag {
            id: "t1".into(),
            name: "climbing".into(),
        },
        ContactTag {
            id: "t2".into(),
            name: "work".into(),
        },
    ])
}

// @internal
#[test]
fn contact_detail_renders_current_tags_with_remove_ids() {
    let screen = tagged_engine().current_screen();
    let items = action_list_items(&screen, "contact_tags")
        .expect("contact_tags ActionList must be present");
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        vec!["climbing", "work"],
        "tag labels render in order"
    );
    assert_eq!(items[0].id, "remove_tag:t1");
    assert_eq!(items[1].id, "remove_tag:t2");
}

// @internal
#[test]
fn contact_detail_with_no_tags_renders_empty_tag_list() {
    let screen = exchanged_engine().current_screen();
    let items = action_list_items(&screen, "contact_tags")
        .expect("contact_tags ActionList must be present even when empty");
    assert!(items.is_empty(), "no tags ⇒ empty list, got {items:?}");
}

// @internal
#[test]
fn contact_detail_renders_add_tag_input() {
    let screen = tagged_engine().current_screen();
    let value = screen.components.iter().find_map(|c| match c {
        Component::TextInput { id, value, .. } if id == "add_tag" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(
        value,
        Some(String::new()),
        "add_tag TextInput present and empty by default"
    );
}

// @internal
#[test]
fn no_suggestions_list_when_query_empty() {
    let screen = tagged_engine().current_screen();
    assert!(
        action_list_items(&screen, "tag_suggestions").is_none(),
        "empty query ⇒ no suggestions list rendered"
    );
}

// @internal
#[test]
fn suggestions_render_as_add_tag_actions_and_echo_query() {
    let mut engine = tagged_engine();
    engine.set_tag_query("cl".into(), vec!["climbing".into(), "climbing-gym".into()]);
    let screen = engine.current_screen();

    let items = action_list_items(&screen, "tag_suggestions")
        .expect("non-empty query ⇒ tag_suggestions list present");
    assert_eq!(
        items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        vec!["add_tag:climbing", "add_tag:climbing-gym"],
        "each suggestion carries an autocomplete-or-create add_tag action"
    );
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        vec!["climbing", "climbing-gym"]
    );

    let value = screen.components.iter().find_map(|c| match c {
        Component::TextInput { id, value, .. } if id == "add_tag" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(value, Some("cl".into()), "input echoes the typed query");
}

// ── Test helpers ────────────────────────────────────────────────────

fn sample_contact_with_email() -> Item {
    Item {
        id: "c1".into(),
        name: "Alice".into(),
        subtitle: Some("alice@example.com".into()),
        initials: "AL".into(),
        status: None,
        actions: vec![],
        a11y: None,
    }
}

fn sample_fields_with_email() -> Vec<Field> {
    vec![Field {
        id: "f1".into(),
        label: "Email".into(),
        value: "alice@example.com".into(),
        field_type: "email".into(),
        icon: "envelope".into(),
        visibility_label: String::new(),
        visibility: UiFieldVisibility::Shown,
        a11y: None,
    }]
}

fn make_detail_engine() -> ContactDetailEngine {
    ContactDetailEngine::new(
        sample_contact_with_email(),
        sample_fields_with_email(),
        String::new(),
    )
}

// ── Personal note tests (Task 7) ─────────────────────────────────────

/// @scenario: navigation :: Add private note to a contact
///
/// Verifies that a saved personal note is surfaced as an editable text
/// component on the ContactDetail screen when it is loaded.
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_personal_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let card = ContactCard::new("Bob");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([2u8; 32], card, shared_key, 0);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // Save a personal note (plain UTF-8 bytes — app layer does not encrypt)
    let note_text = "Met at the Rust conference";
    vauchi
        .save_personal_notes(&bob_id, note_text.as_bytes())
        .unwrap();

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

/// ContactDetail shows empty note when none saved
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_shows_empty_note_when_no_note_saved() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let card = ContactCard::new("Carol");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([3u8; 32], card, shared_key, 0);
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

/// @scenario: navigation :: Add private note to a contact
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_explicit_save_persists_personal_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let card = ContactCard::new("Dave");
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([4u8; 32], card, shared_key, 0);
    let dave_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id.clone(),
    });

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "edit_personal_note".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "edit_personal_note should return UpdateScreen, got {result:?}"
    );

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: "personal_note".into(),
        value: "Colleague at Acme Corp".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged on personal_note should return UpdateScreen, got {result:?}"
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "save_personal_note".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "save_personal_note should return UpdateScreen, got {result:?}"
    );

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

/// @scenario: navigation :: Add private note to a contact's shared field
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
        0,
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([10u8; 32], card, shared_key, 0);
    let bob_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    // Save a field note (plain UTF-8 bytes — same convention as personal_note)
    let note_text = "Work number, call after 9am";
    vauchi
        .save_contact_field_note(&bob_id, &field_id, note_text.as_bytes())
        .unwrap();

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

/// ContactDetail shows empty field note when none saved
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
        0,
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([11u8; 32], card, shared_key, 0);
    let carol_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

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

/// @scenario: navigation :: Add private note to a contact's shared field
#[cfg(feature = "network-rustls")]
// @internal
#[test]
fn contact_detail_explicit_save_persists_field_note() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();

    let mut card = ContactCard::new("Dave");
    let field = vauchi_core::contact_card::ContactField::new(
        vauchi_core::contact_card::FieldType::Phone,
        "Home",
        "+41 79 222 33 44",
        0,
    );
    let field_id = field.id().to_string();
    card.add_field(field).unwrap();
    let shared_key = SymmetricKey::generate();
    let contact = Contact::from_exchange([12u8; 32], card, shared_key, 0);
    let dave_id = contact.id().to_string();
    vauchi.add_contact(contact).unwrap();

    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail {
        contact_id: dave_id.clone(),
    });

    let component_id = format!("field_note:{field_id}");

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: format!("edit_field_note:{field_id}"),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "edit_field_note should return UpdateScreen, got {result:?}"
    );

    let result = engine.handle_action(UserAction::TextChanged {
        component_id: component_id.clone(),
        value: "Prefers text messages".into(),
    });

    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "TextChanged on field_note should return UpdateScreen, got {result:?}"
    );

    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: format!("save_field_note:{field_id}"),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "save_field_note should return UpdateScreen, got {result:?}"
    );

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
fn contact_detail_main_screen_has_no_back_action() {
    // Back is the frontend's core-driven chrome now (gated on can_go_back),
    // not a footer action — 2026-06-05-core-driven-back-chrome. The
    // not-found error screen keeps its own explicit Back.
    let engine = make_detail_engine();
    let screen = engine.current_screen();
    assert!(
        !screen.contextual_actions.iter().any(|a| a.id == "back"),
        "main contact-detail must not offer a footer back action"
    );
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
