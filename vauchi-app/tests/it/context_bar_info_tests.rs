// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The context bar's info slot explains the screen (vauchi/private#479).
//!
//! Core fills it from `screen_info.<screen_id>` in the active locale, with
//! the screen's title standing in for `{name}`, and only while the person
//! keeps "Show help icons" on. A screen without text has no slot, so a shell
//! never draws an icon that opens nothing.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, ContextBar, Event, OverlayKind};

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The bar of the surface the person is on: the last one in the batch,
/// after any companion pane's.
fn context_bar(commands: &[Command]) -> (String, ContextBar) {
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => Some((surface_id.as_str().to_owned(), (**bar).clone())),
            _ => None,
        })
        .expect("the batch must set a context bar")
}

fn present_commands(engine: &mut AppEngine) -> Vec<Command> {
    engine.initial_commands().expect("initial commands")
}

// @scenario: generic_presentation_protocol.feature :: Contextual controls expose four stable roles
#[test]
fn contacts_carries_an_info_action_whose_overlay_says_what_the_screen_is_for() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Contacts);
    let commands = present_commands(&mut engine);
    let (surface_id, bar) = context_bar(&commands);

    let info = bar
        .info
        .expect("Contacts has a description, so the bar offers info");
    assert_eq!(info.label, "Info");
    assert_eq!(info.accessibility_label, "About this screen");

    let opened = engine
        .dispatch(Event::ActionActivated {
            surface_id: vauchi_core::SurfaceId::new(&surface_id).unwrap(),
            interaction_id: info.interaction_id,
        })
        .expect("info activation");
    let overlay = opened
        .iter()
        .find_map(|command| match command {
            Command::PresentOverlay { overlay, .. } => Some(overlay),
            _ => None,
        })
        .expect("info opens an overlay");
    assert_eq!(overlay.kind, OverlayKind::Information);
    assert_eq!(overlay.title.as_deref(), Some("Contacts"));
    let body = overlay.body.as_deref().expect("information carries text");
    assert!(
        body.starts_with("Here are the people you have exchanged cards with"),
        "the body is screen_info.contact_list, got {body:?}"
    );
    assert!(overlay.items.is_empty());
}

// @internal
#[test]
fn the_contact_detail_text_names_the_contact() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let vcf = b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Grace Hopper\r\nEND:VCARD\r\n";
    vauchi.import_contacts_from_vcf(vcf).unwrap();
    let contact_id = vauchi.list_contacts().unwrap()[0].id().to_string();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::ContactDetail { contact_id });

    let (surface_id, bar) = context_bar(&present_commands(&mut engine));
    let info = bar.info.expect("Contact Detail has a description");
    let opened = engine
        .dispatch(Event::ActionActivated {
            surface_id: vauchi_core::SurfaceId::new(&surface_id).unwrap(),
            interaction_id: info.interaction_id,
        })
        .expect("info activation");
    let body = opened
        .iter()
        .find_map(|command| match command {
            Command::PresentOverlay { overlay, .. } => overlay.body.clone(),
            _ => None,
        })
        .expect("information carries text");
    assert!(body.contains("Grace Hopper"), "got {body:?}");
    assert!(
        !body.contains("{name}"),
        "the placeholder must be filled: {body:?}"
    );
}

// @internal
#[test]
fn turning_help_icons_off_removes_the_info_slot_everywhere() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "appearance".into(),
        item_id: "show_help_icons".into(),
    });
    engine.navigate_to(AppScreen::Contacts);

    let (_, bar) = context_bar(&present_commands(&mut engine));
    assert!(bar.info.is_none(), "help is off, so no info slot: {bar:?}");
}

// @internal
#[test]
fn a_screen_without_a_description_offers_no_info_slot() {
    let mut engine = engine_with_identity();
    engine.navigate_to(AppScreen::Tags);

    let (_, bar) = context_bar(&present_commands(&mut engine));
    assert!(bar.info.is_none(), "no screen_info.tags text: {bar:?}");
}
