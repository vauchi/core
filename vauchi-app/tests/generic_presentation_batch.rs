// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core prepares complete generic presentation batches for display shells.

use vauchi_app::ui::AppEngine;
use vauchi_core::{Command, Event, api::Vauchi};

fn initial_batch() -> (AppEngine, Vec<Command>) {
    let mut engine = AppEngine::new(Vauchi::in_memory().expect("in-memory Core"));
    let commands = engine.initial_commands().expect("initial reducer batch");
    (engine, commands)
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn initial_batch_is_a_serializable_generic_surface_transaction() {
    let (_, commands) = initial_batch();
    let json = serde_json::to_string(&commands).expect("serialize generic commands");
    let decoded: Vec<Command> = serde_json::from_str(&json).expect("decode generic commands");

    assert_eq!(decoded, commands);
    let surface = commands.iter().find_map(|command| {
        let Command::ReplaceSurface { surface } = command else {
            return None;
        };
        Some(surface)
    });
    let bar = commands.iter().find_map(|command| {
        let Command::SetContextBar {
            surface_id,
            revision,
            bar,
        } = command
        else {
            return None;
        };
        Some((surface_id, revision, bar))
    });
    let (surface_id, revision, _) = bar.expect("context bar");
    let surface = surface.expect("surface");
    assert_eq!(surface_id, &surface.surface_id);
    assert_eq!(*revision, surface.revision);
}

// @scenario: generic_presentation_protocol.feature :: Contextual controls expose four stable roles
#[test]
fn visible_context_actions_have_accessible_labels() {
    let (_, commands) = initial_batch();
    let bar = commands.iter().find_map(|command| {
        let Command::SetContextBar { bar, .. } = command else {
            return None;
        };
        Some(bar)
    });

    for action in [
        bar.expect("context bar").back.as_ref(),
        bar.expect("context bar").navigation.as_ref(),
        bar.expect("context bar").primary.as_ref(),
        bar.expect("context bar").secondary.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        assert!(!action.label.is_empty());
        assert!(!action.accessibility_label.is_empty());
    }
}

// @scenario: generic_presentation_protocol.feature :: User interaction returns as an opaque event
#[test]
fn an_opaque_visible_action_round_trips_through_the_reducer() {
    let (mut engine, commands) = initial_batch();
    let (surface_id, interaction_id) = commands
        .iter()
        .find_map(|command| {
            let Command::SetContextBar {
                surface_id, bar, ..
            } = command
            else {
                return None;
            };
            bar.primary
                .as_ref()
                .map(|action| (surface_id.clone(), action.interaction_id.clone()))
        })
        .expect("primary action");

    let next = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("reduce opaque action");

    assert!(matches!(
        next.first(),
        Some(Command::ReplaceSurface { surface }) if surface.revision == 2
    ));
}
