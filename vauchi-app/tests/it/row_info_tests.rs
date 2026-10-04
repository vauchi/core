// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A single option can explain itself (vauchi/private#479): a row whose
//! item names an `info_key` carries an info action, and activating it opens
//! an information overlay with the text for that key in the active locale.
//! The same "Show help icons" setting that governs the bar's slot governs
//! these, and an item without text has no action.

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::{
    Command, ContactField, Event, FieldType, OverlayKind, OverlaySpec, PresentationNode,
    PresentationRow, SurfaceId,
};

fn own_field(vauchi: &Vauchi, label: &str) -> String {
    let field = ContactField::new(FieldType::Email, label, &format!("{label}@example.org"), 0);
    let id = field.id().to_string();
    vauchi.add_own_field(field).unwrap();
    id
}

fn engine_on_a_group_with_one_entry() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    own_field(&vauchi, "Home address");
    let family = vauchi.create_group("Family").unwrap().id().to_string();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::GroupDetail { group_id: family });
    engine
}

fn rows(nodes: &[PresentationNode], out: &mut Vec<PresentationRow>) {
    for node in nodes {
        match node {
            PresentationNode::List {
                rows: list_rows, ..
            } => out.extend(list_rows.iter().cloned()),
            PresentationNode::Group { children, .. } => rows(children, out),
            _ => {}
        }
    }
}

/// The rows of the surface the person is on, with its id.
fn surface_rows(commands: &[Command]) -> (SurfaceId, Vec<PresentationRow>) {
    let surface = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("the batch replaces a surface");
    let mut out = Vec::new();
    rows(&surface.nodes, &mut out);
    (surface.surface_id.clone(), out)
}

fn information_overlay(commands: &[Command]) -> Option<OverlaySpec> {
    commands.iter().find_map(|command| match command {
        Command::PresentOverlay { overlay, .. } if overlay.kind == OverlayKind::Information => {
            Some(overlay.clone())
        }
        _ => None,
    })
}

// @internal
#[test]
fn a_group_entry_row_offers_info_that_says_who_sees_the_entry() {
    let mut engine = engine_on_a_group_with_one_entry();
    let commands = engine.initial_commands().expect("initial commands");
    let (surface_id, rows) = surface_rows(&commands);
    let entry = rows
        .iter()
        .find(|row| row.title == "Home address")
        .expect("the entry row");
    let info = entry.info.clone().expect("an entry row carries info");
    assert_eq!(info.label, "Info");
    assert_eq!(info.accessibility_label, "About Home address");

    let opened = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: info.interaction_id,
        })
        .expect("info activation");
    let overlay = information_overlay(&opened).expect("info opens an information overlay");
    assert_eq!(overlay.title.as_deref(), Some("Who sees this entry"));
    let body = overlay.body.as_deref().expect("information carries text");
    assert!(
        body.starts_with("On means the people in this group see this entry"),
        "the body is info.group_entry.body, got {body:?}"
    );
    assert!(overlay.items.is_empty());
    assert_eq!(overlay.close_label.as_deref(), Some("Close"));
    assert!(
        !opened
            .iter()
            .any(|command| matches!(command, Command::PresentAlert { .. })),
        "the text is an overlay, not an alert: {opened:?}"
    );
}

// @internal
#[test]
fn turning_help_icons_off_removes_every_row_info() {
    let mut engine = engine_on_a_group_with_one_entry();
    engine.navigate_to(AppScreen::SettingsAppearance);
    let _ = engine.handle_action(UserAction::SettingsToggled {
        component_id: "appearance".into(),
        item_id: "show_help_icons".into(),
    });
    let group = engine.vauchi().list_groups().unwrap()[0].id().to_string();
    engine.navigate_to(AppScreen::GroupDetail { group_id: group });

    let (_, rows) = surface_rows(&engine.initial_commands().expect("initial commands"));
    let entry = rows
        .iter()
        .find(|row| row.title == "Home address")
        .expect("the entry row");
    assert!(entry.info.is_none(), "help is off, so no info: {entry:?}");
}

/// Settings already named `info_key`s before #479 (the Duress PIN row); the
/// row-level info carries them to the wire with the same overlay.
// @internal
#[test]
fn the_duress_pin_settings_row_opens_its_text_as_an_overlay() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Ada").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::SettingsAdvanced);

    let (surface_id, rows) = surface_rows(&engine.initial_commands().expect("initial commands"));
    let duress = rows
        .iter()
        .find(|row| row.title == "Duress PIN")
        .expect("the Duress PIN row");
    let info = duress.info.clone().expect("the row carries info");
    assert_eq!(info.accessibility_label, "About Duress PIN");

    let opened = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: info.interaction_id,
        })
        .expect("info activation");
    let overlay = information_overlay(&opened).expect("an information overlay");
    assert_eq!(overlay.title.as_deref(), Some("Duress PIN"));
    assert!(
        overlay
            .body
            .as_deref()
            .is_some_and(|body| body.starts_with("A duress PIN opens Vauchi")),
        "{overlay:?}"
    );
}
