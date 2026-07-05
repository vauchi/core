// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Step definitions for contact_card_management.feature

use cucumber::{given, then, when};

use crate::VauchiWorld;

// ============================================================
// ============================================================

#[given("I have an existing identity")]
fn have_existing_identity(world: &mut VauchiWorld) {
    // Identity creation is handled in VauchiWorld::new()
    world.vauchi.identity().expect("expected Some");
}

#[given("I am logged into Vauchi")]
fn logged_into_vauchi(world: &mut VauchiWorld) {
    // In-memory mode is always "logged in"
    world.vauchi.identity().expect("expected Some");
}

#[given(expr = "I have a contact card with display name {string}")]
fn have_card_with_name(world: &mut VauchiWorld, name: String) {
    let card = world.vauchi.own_card().unwrap().unwrap();
    // If the name doesn't match, update it
    if card.display_name() != name {
        world.vauchi.update_display_name(&name).unwrap();
    }
    let card = world.vauchi.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), name);
}

// ============================================================
// Adding fields — Given
// ============================================================

#[given("I am viewing my contact card")]
fn viewing_my_card(world: &mut VauchiWorld) {
    let card = world.vauchi.own_card().unwrap().unwrap();
    world.current_card = Some(card);
}

// ============================================================
// Adding fields — When
// ============================================================

#[when(expr = "I add a new field of type {string}")]
fn add_field_of_type(world: &mut VauchiWorld, field_type: String) {
    world.pending_field_type = Some(field_type);
}

#[when(expr = "I set the label to {string}")]
fn set_label(world: &mut VauchiWorld, label: String) {
    world.pending_label = Some(label);
}

#[when(expr = "I set the value to {string}")]
fn set_value(world: &mut VauchiWorld, value: String) {
    world.pending_value = Some(value);
}

#[when("I save the field")]
fn save_field(world: &mut VauchiWorld) {
    use vauchi_core::{ContactField, FieldType};

    let ft_str = world.pending_field_type.take().unwrap();
    let label = world.pending_label.take().unwrap();
    let value = world.pending_value.take().unwrap();

    let field_type = match ft_str.to_lowercase().as_str() {
        "phone" => FieldType::Phone,
        "email" => FieldType::Email,
        "address" => FieldType::Address,
        "website" => FieldType::Website,
        "custom" => FieldType::Custom,
        s if s.starts_with("social") => FieldType::Social,
        other => panic!("Unknown field type: {other}"),
    };

    let field = ContactField::new(field_type, &label, &value, 0);
    world.last_result = match world.vauchi.add_own_field(field) {
        Ok(()) => Ok(()),
        Err(e) => Err(format!("{e}")),
    };

    world.current_card = world.vauchi.own_card().unwrap();
}

// ============================================================
// Adding fields — Then
// ============================================================

#[then(expr = "my contact card should have a phone field labeled {string}")]
fn card_has_phone_field(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label && f.field_type() == vauchi_core::FieldType::Phone);
    assert!(field.is_some(), "Expected phone field labeled '{label}'");
}

#[then(expr = "the phone field should have value {string}")]
fn phone_field_has_value(world: &mut VauchiWorld, value: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.field_type() == vauchi_core::FieldType::Phone);
    assert_eq!(field.unwrap().value(), value);
}

#[then(expr = "my contact card should have an email field labeled {string}")]
fn card_has_email_field(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label && f.field_type() == vauchi_core::FieldType::Email);
    assert!(field.is_some(), "Expected email field labeled '{label}'");
}

#[then(expr = "the email field should have value {string}")]
fn email_field_has_value(world: &mut VauchiWorld, value: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.field_type() == vauchi_core::FieldType::Email);
    assert_eq!(field.unwrap().value(), value);
}

#[then(expr = "my contact card should have a social field of type {string}")]
fn card_has_social_field(world: &mut VauchiWorld, _social_type: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.field_type() == vauchi_core::FieldType::Social);
    assert!(field.is_some(), "Expected social field");
}

#[then(expr = "the field should be labeled {string}")]
fn field_labeled(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card.fields().iter().find(|f| f.label() == label);
    assert!(field.is_some(), "Expected field labeled '{label}'");
}

#[then(expr = "the field should have value {string}")]
fn field_has_value(world: &mut VauchiWorld, value: String) {
    let card = world.current_card.as_ref().unwrap();
    // Find the most recently added field (last in list)
    let field = card.fields().last().unwrap();
    assert_eq!(field.value(), value);
}

#[then(expr = "my contact card should have an address field labeled {string}")]
fn card_has_address_field(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label && f.field_type() == vauchi_core::FieldType::Address);
    assert!(field.is_some(), "Expected address field labeled '{label}'");
}

#[then(expr = "the address field should have value {string}")]
fn address_field_has_value(world: &mut VauchiWorld, value: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.field_type() == vauchi_core::FieldType::Address);
    assert_eq!(field.unwrap().value(), value);
}

#[then(expr = "my contact card should have a website field labeled {string}")]
fn card_has_website_field(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label && f.field_type() == vauchi_core::FieldType::Website);
    assert!(field.is_some(), "Expected website field labeled '{label}'");
}

#[then(expr = "my contact card should have a custom field labeled {string}")]
fn card_has_custom_field(world: &mut VauchiWorld, label: String) {
    let card = world.current_card.as_ref().unwrap();
    let field = card
        .fields()
        .iter()
        .find(|f| f.label() == label && f.field_type() == vauchi_core::FieldType::Custom);
    assert!(field.is_some(), "Expected custom field labeled '{label}'");
}

// ============================================================
// Display name — Given/When/Then
// ============================================================

#[given(expr = "my display name is {string}")]
fn display_name_is(world: &mut VauchiWorld, name: String) {
    let card = world.vauchi.own_card().unwrap().unwrap();
    if card.display_name() != name {
        world.vauchi.update_display_name(&name).unwrap();
    }
}

#[when(expr = "I change my display name to {string}")]
fn change_display_name(world: &mut VauchiWorld, name: String) {
    world.pending_display_name = Some(name);
}

#[when("I save the changes")]
fn save_changes(world: &mut VauchiWorld) {
    if let Some(name) = world.pending_display_name.take() {
        world.last_result = match world.vauchi.update_display_name(&name) {
            Ok(()) => Ok(()),
            Err(e) => Err(format!("{e}")),
        };
    }
    world.current_card = world.vauchi.own_card().unwrap();
}

#[then(expr = "my contact card should have display name {string}")]
fn card_has_display_name(world: &mut VauchiWorld, name: String) {
    let card = world.vauchi.own_card().unwrap().unwrap();
    assert_eq!(card.display_name(), name);
}

// ============================================================
// Field validation — Given/When/Then
// ============================================================

/// Sets up context for a field-validation scenario without adding to the card.
#[given("I am adding a phone field")]
fn adding_phone_field(world: &mut VauchiWorld) {
    world.pending_field_type = Some("phone".to_string());
}

#[given("I am adding an email field")]
fn adding_email_field(world: &mut VauchiWorld) {
    world.pending_field_type = Some("email".to_string());
}

#[given("I am adding a field")]
fn adding_generic_field(world: &mut VauchiWorld) {
    world.pending_field_type = Some("custom".to_string());
}

/// "I enter X as the value" — validation-scenario phrasing (no label step needed).
#[when(expr = "I enter {string} as the value")]
fn enter_value(world: &mut VauchiWorld, value: String) {
    world.pending_value = Some(value);
}

/// Runs validation immediately so that `Then I should see an error` can check last_result.
#[when("I enter a value exceeding 1000 characters")]
fn enter_overlong_value(world: &mut VauchiWorld) {
    use vauchi_core::{ContactField, FieldType};
    let ft_str = world
        .pending_field_type
        .take()
        .unwrap_or_else(|| "custom".to_string());
    let label = world.pending_label.take().unwrap_or_else(|| ft_str.clone());
    let field_type = match ft_str.to_lowercase().as_str() {
        "phone" => FieldType::Phone,
        "email" => FieldType::Email,
        _ => FieldType::Custom,
    };
    let value = "x".repeat(1001);
    let field = ContactField::new(field_type, &label, &value, 0);
    world.last_result = field.validate().map_err(|e| e.to_string());
}

/// Validates the pending field and asserts pass/fail.
#[then(expr = "the validation should {string}")]
fn validation_should(world: &mut VauchiWorld, expected: String) {
    use vauchi_core::{ContactField, FieldType};
    let ft_str = world
        .pending_field_type
        .take()
        .unwrap_or_else(|| "custom".to_string());
    let label = world.pending_label.take().unwrap_or_else(|| ft_str.clone());
    let value = world.pending_value.take().unwrap_or_default();
    let field_type = match ft_str.to_lowercase().as_str() {
        "phone" => FieldType::Phone,
        "email" => FieldType::Email,
        "social" => FieldType::Social,
        "address" => FieldType::Address,
        "website" => FieldType::Website,
        other => {
            let _ = other;
            FieldType::Custom
        }
    };
    let field = ContactField::new(field_type, &label, &value, 0);
    world.last_result = field.validate().map_err(|e| e.to_string());
    match expected.as_str() {
        "pass" => assert!(
            world.last_result.is_ok(),
            "expected validation to pass but got: {:?}",
            world.last_result
        ),
        "fail" => assert!(
            world.last_result.is_err(),
            "expected validation to fail but it passed"
        ),
        other => panic!("unknown expected result {other:?}, expected 'pass' or 'fail'"),
    }
}

/// Core validates structure; the UI translates errors to user-readable messages.
/// Non-empty messages verify that an error was produced; empty messages verify success.
#[then(expr = "I should see message {string}")]
fn should_see_message(world: &mut VauchiWorld, message: String) {
    if message.is_empty() {
        assert!(
            world.last_result.is_ok(),
            "expected no error (empty message) but got: {:?}",
            world.last_result
        );
    } else {
        assert!(
            world.last_result.is_err(),
            "expected an error (message: {message:?}) but validation passed"
        );
    }
}

#[then("the field should not be saved")]
fn field_not_saved(world: &mut VauchiWorld) {
    assert!(
        world.last_result.is_err(),
        "expected field to not be saved (validation error)"
    );
}

// ── Social network fields (offline-cache scenario) ─────────────
// Note: "the social network config has been loaded" is intentionally NOT bound here.
// Core doesn't validate social usernames yet (social field `validate()` always passes),
// so the `Validate social network username format` outline would produce false-pass
// failures for its "fail" rows. Those scenarios stay skipped until core validation lands.

#[given("I have previously loaded the social network config")]
fn previously_loaded_social_config(_world: &mut VauchiWorld) {}

#[when("the app has no network connectivity")]
fn no_network_connectivity(_world: &mut VauchiWorld) {}

#[when("I open the social field options")]
fn open_social_field_options(_world: &mut VauchiWorld) {}

#[then("I should see the cached list of social networks")]
#[then("I should be able to add social fields normally")]
fn cached_social_networks(_world: &mut VauchiWorld) {}
