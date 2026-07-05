// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Engine-wiring tests for `DuressPinEngine` driven through `AppEngine`
//! (ADR-021 / ADR-043 / CC-24).
//!
//! The duress *behaviour* — the PIN and chosen recipient persist via core,
//! and setup will not complete without a recipient — is core's, not a
//! frontend's. Previously this was only exercised by a TUI humble test,
//! so the contact-picker rework
//! (`2026-07-03-coercion-safety-config-gaps`, which gates completion on a
//! selected recipient) silently broke a downstream frontend build instead
//! of a core test. These tests pin that gate at the engine boundary where
//! it belongs.

use vauchi_app::ui::{ActionResult, AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::ImportSource;
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;

const PIN: &str = "135790";

/// Identity + app password (duress setup is gated on an app password,
/// mirroring the real Settings → Security flow).
fn engine_ready() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine
        .vauchi_mut()
        .setup_app_password("app-password-123")
        .unwrap();
    engine
}

/// Adds a contact and returns the id the picker will present it under.
fn add_contact(engine: &AppEngine, name: &str) -> String {
    let before = contact_ids(engine);
    let contact = Contact::from_import(ContactCard::new(name), ImportSource::VcardFile, None, 0);
    engine.vauchi().add_contact(contact).unwrap();
    contact_ids(engine)
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("newly added contact id")
}

fn contact_ids(engine: &AppEngine) -> Vec<String> {
    engine
        .vauchi()
        .list_contacts()
        .unwrap()
        .iter()
        .map(|c| c.id().to_string())
        .collect()
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

fn type_into(engine: &mut AppEngine, component_id: &str, text: &str) {
    for ch in text.chars() {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: component_id.into(),
            value: ch.to_string(),
        });
    }
}

fn toggle(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ItemToggled {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

/// Overview → EnterPin → ConfirmPin → ConfigureAlerts with matching PINs,
/// leaving the engine parked on the alerts screen (no recipient chosen).
fn advance_to_alerts(engine: &mut AppEngine) {
    let _ = press(engine, "configure");
    type_into(engine, "pin", PIN);
    let _ = press(engine, "continue");
    type_into(engine, "confirm_pin", PIN);
    let _ = press(engine, "continue");
}

// @scenario: duress_mode :: Enable duress password (requires app password)
#[test]
fn duress_setup_persists_pin_and_recipient_via_core() {
    let mut engine = engine_ready();
    let bob = add_contact(&engine, "Bob");
    engine.navigate_to(AppScreen::DuressPin);

    advance_to_alerts(&mut engine);
    let _ = toggle(&mut engine, "recipients", &bob);
    let result = press(&mut engine, "save");

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "save with a recipient completes and navigates back, got {result:?}"
    );
    assert!(
        engine.vauchi().is_duress_enabled().unwrap(),
        "duress PIN must be enabled in storage after the setup flow"
    );
    let settings = engine
        .vauchi()
        .load_duress_settings()
        .unwrap()
        .expect("duress settings persisted");
    assert_eq!(settings.alert_contact_ids, vec![bob]);
}

// @internal — the contact-picker rework gates completion on ≥1 recipient
// (2026-07-03-coercion-safety-config-gaps). Save with none selected must
// not persist; this is the exact regression that silently broke the
// downstream TUI humble test.
#[test]
fn duress_setup_does_not_complete_without_a_recipient() {
    let mut engine = engine_ready();
    add_contact(&engine, "Bob"); // pool is non-empty, but we select no-one
    engine.navigate_to(AppScreen::DuressPin);

    advance_to_alerts(&mut engine);
    let result = press(&mut engine, "save");

    assert!(
        !matches!(result, ActionResult::NavigateTo(_)),
        "save without a recipient must not complete, got {result:?}"
    );
    assert!(
        !engine.vauchi().is_duress_enabled().unwrap(),
        "duress must stay disabled when no recipient was chosen"
    );
}

// @internal — a mismatched confirmation PIN is rejected at the engine and
// nothing persists.
#[test]
fn duress_mismatched_confirmation_is_rejected() {
    let mut engine = engine_ready();
    add_contact(&engine, "Bob");
    engine.navigate_to(AppScreen::DuressPin);

    let _ = press(&mut engine, "configure");
    type_into(&mut engine, "pin", PIN);
    let _ = press(&mut engine, "continue");
    type_into(&mut engine, "confirm_pin", "999999");
    let result = press(&mut engine, "continue");

    // Rejection surfaces as an inline error on the re-rendered confirm
    // screen (Humble contract), not a bare ValidationError to the frontend.
    let confirm_error = match &result {
        ActionResult::UpdateScreen(s) | ActionResult::NavigateTo(s) => {
            s.components.iter().find_map(|c| match c {
                Component::PinInput {
                    id,
                    validation_error,
                    ..
                } if id == "confirm_pin" => Some(validation_error.clone()),
                _ => None,
            })
        }
        _ => None,
    };
    assert!(
        matches!(confirm_error, Some(Some(_))),
        "mismatched confirmation must re-render the confirm PIN with an inline error, got {result:?}"
    );
    assert!(
        !engine.vauchi().is_duress_enabled().unwrap(),
        "a mismatched confirmation must not persist duress"
    );
}
