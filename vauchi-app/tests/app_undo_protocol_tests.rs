// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::{
    Command, Event, ImportSource, InputMode, MotionPreference, PresentationNode, StandardShortcut,
    api::Vauchi, contact::Contact, contact_card::ContactCard,
};

fn app_with_imported_contact() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let contact = Contact::from_import(
        "contact-bob".into(),
        ContactCard::new("Bob"),
        ImportSource::VcardFile,
        None,
        0,
    );
    vauchi.add_contact(contact).expect("contact");
    let mut app = AppEngine::new(vauchi);
    app.set_initial_screen(AppScreen::Contacts);
    app
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Primary action becomes causal Undo
// @scenario: generic_presentation_protocol.feature :: Primary action becomes causal Undo
#[test]
fn secondary_mutation_becomes_primary_undo_and_survives_resizing() {
    let mut app = app_with_imported_contact();
    let initial = app.initial_commands().expect("contacts commands");
    let (surface_id, hide_interaction) = initial
        .iter()
        .find_map(|command| {
            let Command::ReplaceSurface { surface } = command else {
                return None;
            };
            surface.nodes.iter().find_map(|node| {
                let PresentationNode::List { rows, .. } = node else {
                    return None;
                };
                rows.first().and_then(|row| {
                    row.secondary_actions
                        .iter()
                        .find(|action| action.label == "Hide")
                        .map(|action| (surface.surface_id.clone(), action.interaction_id.clone()))
                })
            })
        })
        .expect("hide interaction");

    let hidden = app
        .dispatch(Event::ActionActivated {
            surface_id: surface_id.clone(),
            interaction_id: hide_interaction,
        })
        .expect("hide contact");
    let undo = hidden
        .iter()
        .rev()
        .find_map(|command| {
            let Command::SetContextBar { bar, .. } = command else {
                return None;
            };
            bar.primary.clone()
        })
        .expect("primary undo");
    assert_eq!(undo.label, "Undo");
    assert_eq!(undo.shortcut, Some(StandardShortcut::Undo));

    for width in [500, 900] {
        let resized = app
            .dispatch(Event::PresentationEnvironmentChanged {
                available_width: width,
                available_height: 700,
                input_modes: vec![InputMode::Touch],
                motion: MotionPreference::Full,
            })
            .expect("resize");
        assert!(
            resized
                .iter()
                .all(|command| !matches!(command, Command::SetContextBar { .. }))
        );
    }

    let restored = app
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: undo.interaction_id,
        })
        .expect("undo hide");
    assert!(restored.iter().any(|command| matches!(
        command,
        Command::ReplaceSurface { surface }
            if surface.nodes.iter().any(|node| matches!(
                node,
                PresentationNode::List { rows, .. }
                    if rows.iter().any(|row| row.title == "Bob")
            ))
    )));
    assert!(restored.iter().any(|command| matches!(
        command,
        Command::SetContextBar { bar, .. }
            if bar.primary.as_ref().is_some_and(|action| action.label == "Add Contact")
    )));
}
