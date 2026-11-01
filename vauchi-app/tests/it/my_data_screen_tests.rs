// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The "what we store" surface (private#86): Settings → Privacy & Data →
//! View My Data lists what Vauchi holds on this device, with live counts,
//! what the relay holds and who runs it, that everything is end-to-end
//! encrypted, and where the public security review is.

use vauchi_app::ui::{
    ActionResult, AppEngine, AppScreen, Component, Status, UserAction, WorkflowEngine,
};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{ContactField, FieldType, SymmetricKey};

fn engine_ready() -> AppEngine {
    let mut vauchi: Vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn add_exchanged_contact(engine: &AppEngine, name: &str, key_byte: u8) {
    let contact = Contact::from_exchange(
        [key_byte; 32],
        ContactCard::new(name),
        SymmetricKey::generate(),
        0,
    );
    engine.vauchi().add_contact(contact).unwrap();
}

fn pick(engine: &mut AppEngine, component_id: &str, item_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ListItemSelected {
        component_id: component_id.into(),
        item_id: item_id.into(),
    })
}

fn press(engine: &mut AppEngine, action_id: &str) -> ActionResult {
    engine.handle_action(UserAction::ActionPressed {
        action_id: action_id.into(),
    })
}

/// Settings → Privacy & Data → View My Data, the way a person gets there.
fn open_my_data(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::Settings);
    let result = pick(engine, "privacy", "privacy");
    assert!(
        matches!(result, ActionResult::NavigateTo(_)),
        "the Your Data row opens Privacy & Data, got {result:?}"
    );
    assert_eq!(*engine.current_app_screen(), AppScreen::Privacy);
    let result = pick(engine, "consent_actions", "view_data");
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "my_data"),
        other => panic!("View My Data opens its screen, got {other:?}"),
    }
}

/// (title, detail) of every row of the named InfoPanel.
fn panel_rows(screen: &vauchi_app::ui::ScreenModel, panel_id: &str) -> Vec<(String, String)> {
    screen
        .components
        .iter()
        .find_map(|c| match c {
            Component::InfoPanel { id, items, .. } if id == panel_id => Some(
                items
                    .iter()
                    .map(|item| (item.title.clone(), item.detail.clone()))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no InfoPanel {panel_id} on {}", screen.screen_id))
}

fn detail_of(rows: &[(String, String)], title: &str) -> String {
    rows.iter()
        .find(|(t, _)| t == title)
        .map(|(_, detail)| detail.clone())
        .unwrap_or_else(|| panic!("no row titled {title} in {rows:?}"))
}

// @scenario: privacy_compliance :: View what data is stored locally
#[test]
fn the_what_we_store_surface_is_reachable() {
    let mut engine = engine_ready();
    add_exchanged_contact(&engine, "Bob", 0xB0);
    add_exchanged_contact(&engine, "Carol", 0xC0);
    engine
        .vauchi()
        .add_own_field(ContactField::new(
            FieldType::Email,
            "Work",
            "alice@example.org",
            0,
        ))
        .unwrap();
    open_my_data(&mut engine);

    let screen = engine.current_screen();
    assert_eq!(screen.title, "Your Data");

    let device = panel_rows(&screen, "on_this_device");
    let titles: Vec<&str> = device.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        titles,
        [
            "Identity",
            "Contacts",
            "Devices",
            "Outgoing updates",
            "Settings"
        ],
        "the four buckets the scenario names, plus what is queued to go out"
    );
    assert_eq!(
        detail_of(&device, "Identity"),
        "Your contact card (1 field(s)) and your encryption keys. The keys never leave this device."
    );
    assert_eq!(
        detail_of(&device, "Contacts"),
        "2 people you've exchanged with, with the cards they shared."
    );
    assert_eq!(
        detail_of(&device, "Devices"),
        "1 linked device(s) that hold your identity."
    );
    assert_eq!(
        detail_of(&device, "Outgoing updates"),
        "0 update(s) sealed and waiting to be sent."
    );
    assert_eq!(
        detail_of(&device, "Settings"),
        "Your preferences, including who sees which field."
    );

    let relay = panel_rows(&screen, "on_the_relay");
    let relay_titles: Vec<&str> = relay.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(
        relay_titles,
        ["Sealed envelopes", "Who runs it", "Nowhere else"]
    );
    assert!(
        detail_of(&relay, "Who runs it").contains("one operator runs both"),
        "ADR-037 addendum: the alpha's in-app copy names the single operator"
    );

    let encrypted = screen.components.iter().find_map(|c| match c {
        Component::StatusIndicator {
            id,
            title,
            status,
            status_label,
            ..
        } if id == "encryption_status" => Some((title.clone(), *status, status_label.clone())),
        _ => None,
    });
    assert_eq!(
        encrypted,
        Some((
            "End-to-end encrypted".to_string(),
            Status::Success,
            "Encrypted".to_string()
        ))
    );
}

// @internal
#[test]
fn the_public_security_review_opens_in_the_browser_and_back_returns_to_privacy() {
    let mut engine = engine_ready();
    open_my_data(&mut engine);

    let result = press(&mut engine, "open_security_review");
    assert_eq!(
        result,
        ActionResult::OpenUrl {
            url: "https://vauchi.app/docs/security/previous-analysis".into(),
        }
    );

    let result = press(&mut engine, "cancel");
    match result {
        ActionResult::NavigateTo(screen) => assert_eq!(screen.screen_id, "privacy_settings"),
        other => panic!("back returns to Privacy & Data, got {other:?}"),
    }
}

// @internal
#[test]
fn a_database_file_shows_its_size_on_this_device() {
    let dir = tempfile::tempdir().unwrap();
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"));
    let mut vauchi = Vauchi::new(config).unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    open_my_data(&mut engine);

    let device = panel_rows(&engine.current_screen(), "on_this_device");
    let size = detail_of(&device, "Size on this device");
    assert!(
        size.starts_with("About ") && size.ends_with(" KB, encrypted."),
        "a fresh database is a few dozen kilobytes: {size}"
    );
}

/// An in-memory database has no file, so the size row is left out rather
/// than guessed.
// @internal
#[test]
fn without_a_database_file_the_size_row_is_absent() {
    let mut engine = engine_ready();
    open_my_data(&mut engine);

    let device = panel_rows(&engine.current_screen(), "on_this_device");
    assert!(
        device
            .iter()
            .all(|(title, _)| title != "Size on this device"),
        "{device:?}"
    );
}
