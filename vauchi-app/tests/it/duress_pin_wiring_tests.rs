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

fn start_setup(engine: &mut AppEngine) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: "duress_actions".into(),
        item_id: "set_up".into(),
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
    let _ = start_setup(engine);
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

// @scenario: duress_mode :: Enable duress password (requires app password)
#[test]
fn duress_setup_completes_with_no_contacts_to_alert() {
    let mut engine = engine_ready(); // no contacts at all
    engine.navigate_to(AppScreen::DuressPin);

    advance_to_alerts(&mut engine);
    let result = press(&mut engine, "save");

    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "with an empty contact pool the ≥1-recipient gate is unsatisfiable; blocking here would \
         deny the PIN's decoy protection to every user who has not exchanged yet, got {result:?}"
    );
    assert!(
        engine.vauchi().is_duress_enabled().unwrap(),
        "duress PIN must be enabled even when there is nobody to alert"
    );
    let settings = engine
        .vauchi()
        .load_duress_settings()
        .unwrap()
        .expect("duress settings persisted");
    assert!(
        settings.alert_contact_ids.is_empty(),
        "no recipients were selectable, so none may be persisted"
    );
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

    let _ = start_setup(&mut engine);
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

    // Decoy contact: duress mode must present it as the plausible contact list.
    let decoy_id = "decoy-1".to_string();
    engine
        .vauchi()
        .add_decoy_contact(&decoy_id, "Safe Name", &ContactCard::new("Safe Name"))
        .unwrap();

    let pending_before = engine
        .vauchi()
        .storage()
        .pending()
        .count_all_pending_updates()
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
        .get_all_pending_updates()
        .unwrap();
    assert_eq!(
        pending.len(),
        pending_before + 1,
        "exactly one covert alert must be queued by duress unlock"
    );
    assert_eq!(
        pending[0].contact_id, trusted_id,
        "the queued alert must be addressed to the configured trusted contact"
    );
    let visible = engine.vauchi().list_contacts().unwrap();
    let visible_names: Vec<String> = visible
        .iter()
        .map(|c| c.display_name().to_string())
        .collect();
    assert_eq!(
        visible.len(),
        1,
        "duress mode must show exactly the decoy contact, got {visible_names:?}"
    );
    assert!(
        visible_names.contains(&"Safe Name".to_string()),
        "duress mode must show the decoy contact, got {visible_names:?}"
    );
    let visible_ids: Vec<String> = visible.iter().map(|c| c.id().to_string()).collect();
    assert!(
        !visible_ids.contains(&trusted_id),
        "duress mode lists decoys, never the real contacts"
    );
}

/// Real duress setup (PIN, one alert contact, one decoy), unlocked with the
/// given PIN.
fn unlocked_with(pin: &str) -> AppEngine {
    let mut engine = engine_ready();
    let ally = add_contact(&engine, "Ally");
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    engine
        .vauchi()
        .save_duress_settings(&vauchi_core::types::DuressSettings {
            alert_contact_ids: vec![ally],
            alert_message: "I need help".into(),
            include_location: false,
        })
        .unwrap();
    engine
        .vauchi()
        .add_decoy_contact("decoy-dora", "Dora", &ContactCard::new("Dora"))
        .unwrap();
    let _ = engine.vauchi_mut().authenticate(pin).unwrap();
    engine
}

/// Everything the Duress PIN screen shows, toolbar and body alike, so the
/// test holds whichever layout the screen has (#459 moved its actions
/// into the body).
fn duress_screen_text(engine: &mut AppEngine) -> String {
    let screen = engine.navigate_to(AppScreen::DuressPin);
    let toolbar: Vec<&str> = screen
        .contextual_actions
        .iter()
        .map(|a| a.label.as_str())
        .collect();
    format!("{:?} {:?}", screen.components, toolbar)
}

fn offers_turn_off(text: &str) -> bool {
    text.contains("Disable") || text.contains("Turn off")
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
// ADR-032 / #462: a coercer who opens Settings → Security after a duress
// unlock sees an app with no duress protection, and nothing to disable.
#[test]
fn duress_mode_settings_read_as_not_set_up() {
    let mut normal = unlocked_with("app-password-123");
    let shown = duress_screen_text(&mut normal);
    assert!(
        offers_turn_off(&shown),
        "normal mode can turn it off: {shown}"
    );
    assert!(!shown.contains("not set up"), "{shown}");
    let decoys = normal.navigate_to(AppScreen::DecoyContacts);
    assert!(format!("{:?}", decoys.components).contains("Dora"));

    let mut duress = unlocked_with(PIN);
    let shown = duress_screen_text(&mut duress);
    assert!(shown.contains("Duress PIN not set up"), "{shown}");
    assert!(shown.contains("Set Up PIN"), "{shown}");
    assert!(!offers_turn_off(&shown), "nothing to disable: {shown}");
    let decoys = duress.navigate_to(AppScreen::DecoyContacts);
    assert!(
        !format!("{:?}", decoys.components).contains("Dora"),
        "the decoy set is not named in duress mode"
    );
}

// @scenario: duress_mode :: The duress setup is invisible in duress mode
// #468: after a duress unlock no screen shows the real contacts, groups,
// tags or places. My Card stays real (owner decision 2026-10-01), so its
// entries are not markers here.
#[test]
fn duress_mode_screens_show_no_real_data() {
    let mut engine = engine_ready();
    let real = Contact::from_exchange(
        [7u8; 32],
        ContactCard::new("RealRita"),
        vauchi_core::SymmetricKey::generate(),
        0,
    );
    let rita = real.id().to_string();
    engine.vauchi().add_contact(real).unwrap();
    engine
        .vauchi()
        .add_tag_to_contact(&rita, "RealTagX")
        .unwrap();
    engine
        .vauchi()
        .set_exchange_location(&rita, 47.0, 8.0)
        .unwrap();
    engine
        .vauchi()
        .name_exchange_place(&rita, "RealPlaceY")
        .unwrap();
    let group = engine.vauchi().create_group("RealGroupZ").unwrap();
    engine
        .vauchi()
        .add_contact_to_group(group.id(), &rita)
        .unwrap();
    engine
        .vauchi()
        .add_decoy_contact("decoy-dora", "Dora", &ContactCard::new("Dora"))
        .unwrap();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    let _ = engine.vauchi_mut().authenticate(PIN).unwrap();

    let markers = ["RealRita", "RealTagX", "RealPlaceY", "RealGroupZ"];
    let mut screens = engine.available_screens();
    screens.push(AppScreen::ContactDetail { contact_id: rita });
    screens.push(AppScreen::GroupDetail {
        group_id: group.id().to_string(),
    });
    let mut leaks = Vec::new();
    for screen in screens {
        let label = format!("{screen:?}");
        let model = engine.navigate_to(screen);
        let shown = format!(
            "{:?} {:?} {:?}",
            model.title, model.components, model.contextual_actions
        );
        for marker in markers {
            if shown.contains(marker) {
                leaks.push(format!("{label}: {marker}"));
            }
        }
    }
    assert!(leaks.is_empty(), "real data in duress mode: {leaks:?}");
}

// The privacy screen works under the duress PIN like every other screen
// (ADR-032), so its export reads the decoy book (vauchi/private#576).
// @scenario: duress_mode :: Cannot access real contacts from duress mode
#[test]
fn duress_mode_gdpr_export_holds_no_real_contacts() {
    let mut engine = engine_ready();
    let real = Contact::from_exchange(
        [9u8; 32],
        ContactCard::new("RealRita"),
        vauchi_core::SymmetricKey::generate(),
        0,
    );
    let real_fingerprint = real.fingerprint();
    engine.vauchi().add_contact(real).unwrap();
    engine
        .vauchi()
        .add_decoy_contact("decoy-dora", "Dora", &ContactCard::new("Dora"))
        .unwrap();
    engine.vauchi_mut().setup_duress_password(PIN).unwrap();
    let _ = engine.vauchi_mut().authenticate(PIN).unwrap();

    engine.navigate_to(AppScreen::Privacy);
    let json = match press(&mut engine, "export") {
        ActionResult::GdprExportComplete { json } => json,
        other => panic!("expected GdprExportComplete, got {other:?}"),
    };

    assert!(
        !json.contains("RealRita"),
        "export holds a real contact: {json}"
    );
    assert!(
        !json.contains(&real_fingerprint),
        "export holds a real fingerprint: {json}"
    );
    assert!(json.contains("Dora"), "decoy missing from export: {json}");
}
