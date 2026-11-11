// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The native status item (system tray) carries Core's copy through the same
//! wire as the application menu, and only to a shell that can show one.

use serde_json::json;
use vauchi_app::ui::{AppEngine, AppScreen, RenderContext};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, StatusItemSpec};

fn engine() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Settings);
    engine.initial_commands().unwrap();
    engine
}

fn availability(engine: &mut AppEngine, available: bool) -> Vec<Command> {
    let event = serde_json::from_value(json!({
        "StatusItemAvailabilityChanged": {"available": available}
    }))
    .expect("status item availability is a generic wire event");
    engine.dispatch(event).unwrap()
}

fn status_items(commands: &[Command]) -> Vec<(String, u64, Option<StatusItemSpec>)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::SetStatusItem {
                surface_id,
                revision,
                item,
            } => Some((surface_id.as_str().to_owned(), *revision, item.clone())),
            _ => None,
        })
        .collect()
}

fn surface_revision(commands: &[Command]) -> u64 {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.revision),
            _ => None,
        })
        .expect("Core renders the current surface")
}

// @scenario: generic_presentation_protocol.feature :: Core chooses a capability-compatible presentation
#[test]
fn the_status_item_is_opt_in_and_can_be_removed() {
    let mut engine = engine();
    assert!(status_items(&engine.initial_commands().unwrap()).is_empty());

    let shown = availability(&mut engine, true);
    let items = status_items(&shown);
    assert_eq!(items.len(), 1);
    let (surface, revision, item) = &items[0];
    assert_eq!(surface, AppScreen::Settings.screen_id());
    assert_eq!(*revision, surface_revision(&shown));
    assert_eq!(
        item.as_ref().map(|spec| spec.items.len()),
        Some(0),
        "the tray holds only the shell's show-window item and Quit"
    );

    let removed = status_items(&availability(&mut engine, false));
    assert_eq!(removed.len(), 1);
    assert_eq!(removed[0].2, None);
    assert!(status_items(&engine.initial_commands().unwrap()).is_empty());
}

// @scenario: generic_presentation_protocol.feature :: Core chooses a capability-compatible presentation
#[test]
fn the_status_item_copy_comes_from_runtime_content() {
    vauchi_app::i18n::load_locale_from_bytes(
        "qjb",
        br#"{
        "app.name":"Downloaded name",
        "platform.tray_show":"Downloaded show"
    }"#,
    )
    .unwrap();
    let mut engine = engine();
    engine.set_render_context(RenderContext {
        locale: Some("qjb".into()),
        ..Default::default()
    });
    let items = status_items(&availability(&mut engine, true));
    let spec = items[0].2.clone().expect("Core shows the status item");
    assert_eq!(spec.tooltip, "Downloaded name");
    assert_eq!(spec.show_window_label, "Downloaded show");
}

// @scenario: generic_presentation_protocol.feature :: Core chooses a capability-compatible presentation
#[test]
fn the_status_item_follows_each_new_surface() {
    let mut engine = engine();
    availability(&mut engine, true);
    engine.navigate_to(AppScreen::Contacts);
    let commands = engine.initial_commands().unwrap();
    let items = status_items(&commands);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].0, AppScreen::Contacts.screen_id());
    assert_eq!(items[0].1, surface_revision(&commands));
    assert!(items[0].2.is_some());
}
