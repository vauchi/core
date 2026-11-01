// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The emergency broadcast screen (ADR-036, private#895): Settings →
//! Security → Emergency Broadcast picks up to ten trusted contacts, the
//! alert message and the location choice, and sends the alert after a
//! question that closes by itself. Core had the broadcast and only the CLI
//! could reach it; this screen is what every shell renders.

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, EmergencyBroadcastEngine, ToggleItem,
    TrustedContactChoice, UserAction, WorkflowEngine,
};
use vauchi_core::SymmetricKey;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::Clock;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::emergency::DEFAULT_EMERGENCY_MESSAGE;

const APP_PASSWORD: &str = "app-password-123";

/// A clock the test moves by hand: Core's `FakeClock` is behind the
/// `testing` feature, which the lint build of this crate does not enable.
struct TestClock(Mutex<SystemTime>);

impl TestClock {
    fn at(unix_secs: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            UNIX_EPOCH + Duration::from_secs(unix_secs),
        )))
    }

    fn advance(&self, d: Duration) {
        let mut now = self.0.lock().unwrap();
        *now += d;
    }
}

impl Clock for TestClock {
    fn now(&self) -> SystemTime {
        *self.0.lock().unwrap()
    }
}

fn engine_ready() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// An exchanged contact: it has a public key, so an alert can be sealed
/// for it (an imported contact has none and is skipped by Core).
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

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

/// A tap on a `ButtonList` button arrives as `ListItemSelected`.
fn pick(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn toggle(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ItemToggled {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn open_from_settings(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::SettingsAdvanced);
    let result = engine.handle_action(UserAction::ListItemSelected {
        component_id: "security".into(),
        item_id: "emergency_broadcast".into(),
    });
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "the Security row opens the screen, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::EmergencyBroadcast);
}

fn toggle_items(screen: &vauchi_app::ui::ScreenModel, id: &str) -> Vec<ToggleItem> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::ToggleList {
                id: list_id, items, ..
            } if list_id == id => Some(items.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no toggle list {id} on {}", screen.screen_id))
}

fn inline_confirm(screen: &vauchi_app::ui::ScreenModel) -> Option<(String, String)> {
    screen.components.iter().find_map(|c| match c {
        Component::InlineConfirm {
            warning,
            confirm_action_id,
            ..
        } => Some((warning.clone(), confirm_action_id.clone())),
        _ => None,
    })
}

fn status(screen: &vauchi_app::ui::ScreenModel) -> (String, Option<String>) {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::StatusIndicator {
                id, title, detail, ..
            } if id == "status" => Some((title.clone(), detail.clone())),
            _ => None,
        })
        .expect("the screen carries its status")
}

fn texts(screen: &vauchi_app::ui::ScreenModel) -> Vec<String> {
    screen
        .components
        .iter()
        .filter_map(|c| match c {
            Component::Text { content, .. } => Some(content.clone()),
            _ => None,
        })
        .collect()
}

fn configured_engine(contacts: usize) -> EmergencyBroadcastEngine {
    let choices = (0..contacts)
        .map(|n| TrustedContactChoice {
            id: format!("contact-{n}"),
            display_name: format!("Contact {n}"),
            selected: true,
        })
        .collect();
    EmergencyBroadcastEngine::new(
        choices,
        DEFAULT_EMERGENCY_MESSAGE.into(),
        false,
        true,
        Locale::ENGLISH,
    )
}

// @scenario: emergency_broadcast :: Send emergency broadcast from app
#[test]
fn sending_an_emergency_broadcast_is_reachable() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    open_from_settings(&mut engine);

    let _ = toggle(&mut engine, "trusted_contacts", &bob);
    let _ = pick(&mut engine, "config_actions", "save_emergency_config");
    let config = engine
        .vauchi()
        .load_emergency_config()
        .unwrap()
        .expect("saving configures the broadcast");
    assert_eq!(config.trusted_contact_ids, vec![bob.clone()]);

    let _ = pick(&mut engine, "send_actions", "send_alert");
    let (_, confirm_id) = inline_confirm(&engine.current_screen()).expect("sending asks first");
    let result = press(&mut engine, &confirm_id);

    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "{result:?}"
    );
    let (_, detail) = status(&engine.current_screen());
    assert_eq!(
        detail.as_deref(),
        Some("Emergency broadcast sent to 1 contacts")
    );
    let queued = engine
        .vauchi()
        .storage()
        .pending()
        .get_pending_updates(&bob)
        .unwrap();
    assert_eq!(queued.len(), 1, "one disguised alert is queued for Bob");
    assert_eq!(queued[0].update_type, "card_delta");
}

// @scenario: emergency_broadcast :: Emergency broadcast with confirmation
#[test]
fn the_send_button_asks_first_and_the_question_closes_after_five_seconds() {
    let mut engine = configured_engine(5);
    assert!(inline_confirm(&engine.current_screen()).is_none());

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "send_actions".into(),
        item_id: "send_alert".into(),
    });
    let (warning, confirm_id) =
        inline_confirm(&engine.current_screen()).expect("the question is shown");
    assert!(
        warning.starts_with("Send alert to 5 contacts?"),
        "the question names the count: {warning}"
    );
    assert_eq!(confirm_id, "confirm_send_alert");

    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel_send_alert".into(),
    });
    assert!(
        inline_confirm(&engine.current_screen()).is_none(),
        "cancel closes the question"
    );

    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "send_actions".into(),
        item_id: "send_alert".into(),
    });
    let _ = engine.tick(1_000);
    let _ = engine.tick(1_004);
    assert!(
        inline_confirm(&engine.current_screen()).is_some(),
        "the question stays for five seconds"
    );
    let _ = engine.tick(1_005);
    assert!(
        inline_confirm(&engine.current_screen()).is_none(),
        "the question closes by itself after five seconds"
    );
}

// @scenario: emergency_broadcast :: Rate limiting emergency broadcasts
#[test]
fn a_repeat_within_a_minute_is_refused_with_the_seconds_left() {
    let clock = TestClock::at(1_700_000_000);
    let mut vauchi = Vauchi::in_memory_with_clock(clock.clone()).unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    engine
        .vauchi_mut()
        .configure_emergency_broadcast(vec![bob], DEFAULT_EMERGENCY_MESSAGE.into(), false)
        .unwrap();
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = pick(&mut engine, "send_actions", "send_alert");
    let _ = press(&mut engine, "confirm_send_alert");
    let refused = pick(&mut engine, "send_actions", "send_alert");
    match refused {
        ActionResult::ShowAlert { message, .. } => assert!(
            message.contains("60 seconds"),
            "the refusal says how long to wait: {message}"
        ),
        other => panic!("a repeat within a minute is refused with the seconds left, got {other:?}"),
    }
    assert!(inline_confirm(&engine.current_screen()).is_none());

    clock.advance(Duration::from_secs(60));
    let _ = pick(&mut engine, "send_actions", "send_alert");
    assert!(
        inline_confirm(&engine.current_screen()).is_some(),
        "after the minute the question is asked again"
    );
}

// @scenario: emergency_broadcast :: Configure emergency broadcast contacts
#[test]
fn an_eleventh_trusted_contact_is_refused() {
    let choices = (0..11)
        .map(|n| TrustedContactChoice {
            id: format!("contact-{n}"),
            display_name: format!("Contact {n}"),
            selected: n < 10,
        })
        .collect();
    let mut engine = EmergencyBroadcastEngine::new(
        choices,
        DEFAULT_EMERGENCY_MESSAGE.into(),
        false,
        false,
        Locale::ENGLISH,
    );

    let _ = engine.handle_action(UserAction::ItemToggled {
        component_id: "trusted_contacts".into(),
        item_id: "contact-10".into(),
    });

    let screen = engine.current_screen();
    let selected = toggle_items(&screen, "trusted_contacts")
        .iter()
        .filter(|i| i.selected)
        .count();
    assert_eq!(selected, 10, "the eleventh stays unselected");
    assert!(
        texts(&screen)
            .iter()
            .any(|t| t == "Maximum 10 trusted contacts"),
        "the screen says why: {:?}",
        texts(&screen)
    );
}

// @scenario: emergency_broadcast :: Configure alert message
// @scenario: emergency_broadcast :: Configure location sharing for alerts
#[test]
fn saving_keeps_the_contacts_the_message_and_the_location_choice() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    let carol = add_exchanged_contact(&engine, "Carol", 0xC0);
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let screen = engine.current_screen();
    let (title, _) = status(&screen);
    assert_eq!(title, "Emergency broadcast not set up");
    let message = screen.components.iter().find_map(|c| match c {
        Component::TextInput { id, value, .. } if id == "message" => Some(value.clone()),
        _ => None,
    });
    assert_eq!(
        message.as_deref(),
        Some(DEFAULT_EMERGENCY_MESSAGE),
        "the default message is offered"
    );
    let location = toggle_items(&screen, "options");
    assert_eq!(location.len(), 1);
    assert!(!location[0].selected, "location sharing is off by default");

    let _ = toggle(&mut engine, "trusted_contacts", &bob);
    let _ = toggle(&mut engine, "trusted_contacts", &carol);
    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "message".into(),
        value: "Call my sister.".into(),
    });
    let _ = toggle(&mut engine, "options", "include_location");
    let _ = pick(&mut engine, "config_actions", "save_emergency_config");

    let config = engine
        .vauchi()
        .load_emergency_config()
        .unwrap()
        .expect("saved");
    assert_eq!(config.trusted_contact_ids, vec![bob, carol]);
    assert_eq!(config.message, "Call my sister.");
    assert!(config.include_location);
    let (title, _) = status(&engine.current_screen());
    assert_eq!(title, "Emergency broadcast configured");
}

// @internal
#[test]
fn saving_without_a_trusted_contact_is_refused() {
    let mut engine = engine_ready();
    add_exchanged_contact(&engine, "Bob", 0xB0);
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = pick(&mut engine, "config_actions", "save_emergency_config");

    assert!(engine.vauchi().load_emergency_config().unwrap().is_none());
    assert!(
        texts(&engine.current_screen())
            .iter()
            .any(|t| t == "Add at least one trusted contact"),
        "{:?}",
        texts(&engine.current_screen())
    );
}

// @internal
#[test]
fn disabling_asks_then_forgets_the_configuration() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    engine
        .vauchi_mut()
        .configure_emergency_broadcast(vec![bob], DEFAULT_EMERGENCY_MESSAGE.into(), false)
        .unwrap();
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = pick(&mut engine, "config_actions", "disable_emergency_broadcast");
    let (_, confirm_id) = inline_confirm(&engine.current_screen()).expect("disabling asks first");
    assert!(engine.vauchi().load_emergency_config().unwrap().is_some());
    let _ = press(&mut engine, &confirm_id);

    assert!(engine.vauchi().load_emergency_config().unwrap().is_none());
    let (title, _) = status(&engine.current_screen());
    assert_eq!(title, "Emergency broadcast not set up");
}

// @internal — #781: who receives the alerts changes only after the session
// PIN, like the duress PIN; sending never waits.
#[test]
fn a_configuration_change_waits_for_the_session_pin_when_a_password_is_set() {
    let mut engine = engine_ready();
    engine
        .vauchi_mut()
        .setup_app_password(APP_PASSWORD)
        .unwrap();
    engine.vauchi_mut().authenticate(APP_PASSWORD).unwrap();
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let _ = toggle(&mut engine, "trusted_contacts", &bob);
    let _ = pick(&mut engine, "config_actions", "save_emergency_config");
    assert!(
        engine.vauchi().load_emergency_config().unwrap().is_none(),
        "nothing is saved before the PIN"
    );
    assert_eq!(
        engine.current_screen().screen_id,
        "confirm_session_pin",
        "the session PIN step stands in for the screen"
    );

    let _ = engine.handle_action(UserAction::TextChanged {
        component_id: "pin".into(),
        value: APP_PASSWORD.into(),
    });
    let _ = press(&mut engine, "confirm_pin");

    let config = engine
        .vauchi()
        .load_emergency_config()
        .unwrap()
        .expect("saved after the PIN");
    assert_eq!(config.trusted_contact_ids, vec![bob]);
    assert_eq!(*engine.current_app_screen(), AppScreen::EmergencyBroadcast);
}

// @internal
#[test]
fn blocked_contacts_are_not_offered_as_trusted_contacts() {
    let mut engine = engine_ready();
    let bob = add_exchanged_contact(&engine, "Bob", 0xB0);
    let carol = add_exchanged_contact(&engine, "Carol", 0xC0);
    engine.vauchi().block_contact(&bob).unwrap();
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let ids: Vec<String> = toggle_items(&engine.current_screen(), "trusted_contacts")
        .into_iter()
        .map(|i| i.id)
        .collect();
    assert_eq!(ids, vec![carol]);
}

// @internal
#[test]
fn without_contacts_the_screen_says_to_exchange_first() {
    let mut engine = engine_ready();
    engine.navigate_to(AppScreen::EmergencyBroadcast);

    let screen = engine.current_screen();
    assert!(
        !screen
            .components
            .iter()
            .any(|c| matches!(c, Component::ToggleList { id, .. } if id == "trusted_contacts")),
        "no empty picker"
    );
    assert!(
        texts(&screen)
            .iter()
            .any(|t| t.starts_with("Exchange a card with someone first.")),
        "{:?}",
        texts(&screen)
    );
}

// @internal
#[test]
fn the_screen_opens_in_the_active_locale() {
    super::i18n_support::load_german();
    let engine = EmergencyBroadcastEngine::new(
        Vec::new(),
        DEFAULT_EMERGENCY_MESSAGE.into(),
        false,
        false,
        Locale::GERMAN,
    );
    let screen = engine.current_screen();
    assert_eq!(screen.title, "Notfallmeldung");
    assert_eq!(status(&screen).0, "Notfallmeldung nicht eingerichtet");
}
