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
    let contact = Contact::from_import(
        format!("contact-{name}"),
        ContactCard::new(name),
        ImportSource::VcardFile,
        None,
        0,
    );
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

// @scenario: duress_mode :: Duress credential shows decoy contacts
// ADR-032: the duress PIN unlocks into DECOY mode — silent alert queued,
// decoy contacts shown, the app plausibly normal to the coercer. A wipe on
// duress unlock is maximally visible AND destroys the just-queued covert
// alerts before any sync can deliver them.
#[test]
fn duress_unlock_enters_decoy_mode_without_wiping() {
    let mut engine = engine_ready();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();

    // Trusted exchanged contact with a ratchet so the covert alert queues.
    let shared = vauchi_core::SymmetricKey::generate();
    let trusted = Contact::from_exchange([5u8; 32], ContactCard::new("Ally"), shared.clone(), 0);
    let trusted_id = trusted.id().to_string();
    engine.vauchi().add_contact(trusted).unwrap();
    let dh = vauchi_core::exchange::X3DHKeyPair::generate();
    engine
        .vauchi()
        .create_ratchet_as_initiator(&trusted_id, &shared, *dh.public_key())
        .unwrap();
    engine
        .vauchi()
        .save_duress_settings(&vauchi_core::types::DuressSettings {
            alert_contact_ids: vec![trusted_id.clone()],
            alert_message: "help".into(),
            include_location: false,
        })
        .unwrap();

    engine.set_initial_screen(AppScreen::Lock);
    // The lock screen's TextInput carries the full value per TextChanged
    // (unlike the setup flow's per-digit PinInput).
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: PIN.into(),
    });
    let result = press(&mut engine, "unlock");

    assert!(
        !matches!(result, ActionResult::WipeComplete),
        "duress unlock must NOT wipe (ADR-032 decoy mode), got {result:?}"
    );
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "duress unlock proceeds into the (decoy) app, got {result:?}"
    );
    assert!(
        engine.vauchi().has_identity(),
        "storage survives a duress unlock"
    );
    let pending = engine
        .vauchi()
        .storage()
        .pending()
        .count_all_pending_updates()
        .unwrap();
    assert!(
        pending > 0,
        "the covert alert stays queued for delivery after unlock"
    );
    let visible: Vec<String> = contact_ids(&engine);
    assert!(
        !visible.contains(&trusted_id),
        "duress mode lists decoys, never the real contacts"
    );
}
