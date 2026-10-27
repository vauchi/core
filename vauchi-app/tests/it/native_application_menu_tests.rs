// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Native menus carry prepared copy and opaque actions through the same wire.

use serde_json::{Value, json};
use vauchi_app::ui::{AppEngine, AppScreen, RenderContext};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event};

mod safety;

const VCARD: &[u8] = b"BEGIN:VCARD\r\nVERSION:3.0\r\nUID:menu-bob\r\nFN:Bob\r\nEND:VCARD\r\n";

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
        "ApplicationMenuAvailabilityChanged": {"available": available}
    }))
    .expect("native menu availability is a generic wire event");
    engine.dispatch(event).unwrap()
}

fn menu(commands: &[Command]) -> Value {
    commands
        .iter()
        .find_map(|command| {
            serde_json::to_value(command)
                .unwrap()
                .get("SetApplicationMenu")
                .cloned()
        })
        .expect("Core supplies the native menu")
}

fn activation(menu: &Value) -> Event {
    serde_json::from_value(json!({"ActionActivated": {
        "surface_id": menu["surface_id"],
        "interaction_id": menu["menus"][0]["items"][0]["interaction_id"]
    }}))
    .unwrap()
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn menus_are_opt_in_and_can_be_removed() {
    let mut engine = engine();
    assert!(engine.initial_commands().unwrap().iter().all(|command| {
        !serde_json::to_value(command)
            .unwrap()
            .as_object()
            .is_some_and(|object| object.contains_key("SetApplicationMenu"))
    }));
    let shown = menu(&availability(&mut engine, true));
    assert_eq!(shown["menus"].as_array().unwrap().len(), 1);
    assert_eq!(menu(&availability(&mut engine, false))["menus"], json!([]));
}

// @scenario: core_file_picker :: import_contacts action emits FilePickFromUser
#[test]
fn menu_copy_comes_from_runtime_content_and_activation_is_opaque() {
    vauchi_app::i18n::load_locale_from_bytes(
        "qja",
        br#"{
        "menu.file":"Downloaded files",
        "menu.import_contacts":"Downloaded import"
    }"#,
    )
    .unwrap();
    let mut engine = engine();
    engine.set_render_context(RenderContext {
        locale: Some("qja".into()),
        ..Default::default()
    });
    let shown = menu(&availability(&mut engine, true));
    assert_eq!(shown["menus"][0]["label"], "Downloaded files");
    assert_eq!(shown["menus"][0]["role"], "file");
    let item = &shown["menus"][0]["items"][0];
    assert_eq!(item["label"], "Downloaded import");
    assert_eq!(item["enabled"], true);
    assert_eq!(item["shortcut"], "import");
    let commands = engine.dispatch(activation(&shown)).unwrap();
    let pickers: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::FilePickFromUser {
                accepted_extensions,
                ..
            } => Some(accepted_extensions.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(pickers, vec![vec!["vcf", "vcard"]]);
    let picker = commands
        .iter()
        .find_map(|command| {
            serde_json::to_value(command)
                .unwrap()
                .get("FilePickFromUser")
                .cloned()
        })
        .unwrap();
    assert_eq!(picker["max_bytes"], 32 * 1024 * 1024);
    assert_eq!(menu(&commands)["menus"][0]["items"][0]["enabled"], false);
}

// @scenario: core_file_picker :: FilePickedFromUser on Contacts imports vCard
#[test]
fn global_import_keeps_the_originating_screen_and_prepares_results() {
    let mut engine = engine();
    let shown = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&shown)).unwrap();
    let commands = engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "contacts.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 1);
    let surfaces: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(surfaces, vec![AppScreen::Settings.screen_id()]);
    let messages: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::ShowToast { toast } => Some(toast.message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec!["1 contact(s) imported"]);
    assert_eq!(menu(&commands)["menus"][0]["items"][0]["enabled"], true);
}

// @scenario: core_file_picker :: cancellation produces no result
#[test]
fn cancelling_clears_the_global_import_request() {
    let mut engine = engine();
    let shown = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&shown)).unwrap();
    let commands = engine.dispatch(Event::FilePickCancelledByUser).unwrap();
    assert_eq!(menu(&commands)["menus"][0]["items"][0]["enabled"], true);
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "late.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn a_locked_session_disables_import_and_rejects_late_picker_bytes() {
    let mut engine = engine();
    let shown = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&shown)).unwrap();
    engine.navigate_to(AppScreen::Lock);
    let locked = menu(&engine.initial_commands().unwrap());
    assert_eq!(locked["menus"][0]["items"][0]["enabled"], false);
    let error = engine.dispatch(activation(&locked)).unwrap_err();
    assert_eq!(error.kind(), "contextual");
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "late.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn stale_menu_actions_cannot_start_an_import() {
    let mut engine = engine();
    let old = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&old)).unwrap();
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "contacts.vcf".into(),
        })
        .unwrap();
    let current = menu(&engine.initial_commands().unwrap());
    assert_ne!(
        old["menus"][0]["items"][0]["interaction_id"],
        current["menus"][0]["items"][0]["interaction_id"]
    );
    let error = engine.dispatch(activation(&old)).unwrap_err();
    assert_eq!(error.kind(), "contextual");
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 1);
}

// @scenario: core_file_picker :: cancellation produces no result
#[test]
fn file_read_failure_clears_the_pending_import() {
    let mut engine = engine();
    let shown = menu(&availability(&mut engine, true));
    engine.dispatch(activation(&shown)).unwrap();
    let commands = engine
        .dispatch(Event::HardwareError {
            transport: "file_picker".into(),
            error: "Read failed".into(),
        })
        .unwrap();
    assert_eq!(menu(&commands)["menus"][0]["items"][0]["enabled"], true);
    let messages: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::PresentAlert { alert } => Some(alert.message.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec!["Read failed"]);
    engine
        .dispatch(Event::FilePickedFromUser {
            bytes: VCARD.to_vec(),
            filename: "late.vcf".into(),
        })
        .unwrap();
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}

// @scenario: generic_presentation_protocol.feature :: Invalid boundary input fails safely
#[test]
fn import_is_disabled_before_identity_creation() {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let shown = menu(&availability(&mut engine, true));
    assert_eq!(shown["menus"][0]["items"][0]["enabled"], false);
    assert_eq!(
        engine.dispatch(activation(&shown)).unwrap_err().kind(),
        "contextual"
    );
    assert_eq!(engine.vauchi().list_contacts().unwrap().len(), 0);
}
