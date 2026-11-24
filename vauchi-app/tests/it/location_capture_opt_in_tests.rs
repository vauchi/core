// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Remembering where a contact was met is opt-in and off by default
//! (owner, 2026-11-24): an exchange asks the phone for its location only
//! once the person turns on Settings › Privacy › "Remember where I met
//! people". Until then the FAQ's "no location data is collected" was false
//! for every exchange (vauchi/private#1220).

use vauchi_app::ui::{
    AppEngine, AppScreen, Component, SettingsItemKind, UserAction, WorkflowEngine,
};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event};

const TOGGLE: &str = "remember_where_met";

fn engine_with_contact() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Me").unwrap();
    vauchi
        .import_contacts_from_vcf(b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Bob\r\nEND:VCARD\r\n")
        .unwrap();
    let cid = vauchi.list_contacts().unwrap()[0].id().to_string();
    (AppEngine::new(vauchi), cid)
}

fn turn_on(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::Settings);
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "privacy".into(),
        item_id: TOGGLE.into(),
    });
}

fn asks_for_location(engine: &mut AppEngine) -> bool {
    engine
        .drain_pending_commands()
        .iter()
        .any(|command| matches!(command, Command::LocationRequest { .. }))
}

/// The toggle as the Settings screen on display shows it.
fn the_toggle(engine: &AppEngine) -> Option<bool> {
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::SettingsGroup { items, .. } => {
                items.into_iter().find_map(|item| match item.kind {
                    SettingsItemKind::Toggle { enabled } if item.id == TOGGLE => Some(enabled),
                    _ => None,
                })
            }
            _ => None,
        })
}

// @internal
#[test]
fn by_default_an_exchange_asks_for_no_location_and_records_none() {
    let (mut engine, cid) = engine_with_contact();

    engine.request_exchange_location(cid.clone());

    assert!(
        !asks_for_location(&mut engine),
        "no location may be asked for by default"
    );
    let _ = engine.handle_hardware_event(Event::LocationResult {
        latitude: 47.37,
        longitude: 8.54,
        accuracy_meters: Some(12.0),
    });
    assert!(engine.vauchi().exchange_location(&cid).unwrap().is_none());
}

// @internal
#[test]
fn turned_on_an_exchange_asks_for_the_location() {
    let (mut engine, cid) = engine_with_contact();
    turn_on(&mut engine);

    engine.request_exchange_location(cid);

    assert!(asks_for_location(&mut engine));
}

// @internal
#[test]
fn the_choice_is_kept_with_the_other_settings() {
    let (mut engine, _) = engine_with_contact();
    turn_on(&mut engine);

    assert!(engine.vauchi().config().remember_where_met);
    assert!(
        engine
            .vauchi()
            .load_settings_flags()
            .unwrap()
            .remember_where_met
    );
}

// @internal
#[test]
fn settings_offers_the_choice_off_until_turned_on() {
    let (mut engine, _) = engine_with_contact();
    engine.navigate_to(AppScreen::Settings);
    assert_eq!(the_toggle(&engine), Some(false));

    turn_on(&mut engine);

    assert_eq!(the_toggle(&engine), Some(true));
}
