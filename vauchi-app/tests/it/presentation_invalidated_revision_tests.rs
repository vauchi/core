// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A shell sends `PresentationInvalidated` when content arrives after
//! start, such as the locale catalogue on a fresh install. Core rebuilds the
//! screen, and on the onboarding name page the rebuilt engine starts again
//! at Welcome. Welcome then went out at the name page's revision, so a focus
//! end for the name field that was already on its way read as current and
//! was refused as an unknown binding: the person saw "Something went wrong"
//! on Welcome (vauchi/private#1339, Pixel 3a, 2026-11-22).

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::{BindingId, Command, Event, PresentationNode, SurfaceId};

fn shown_surface(commands: &[Command]) -> (SurfaceId, u64, Vec<PresentationNode>) {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some((
                surface.surface_id.clone(),
                surface.revision,
                surface.nodes.clone(),
            )),
            _ => None,
        })
        .expect("the batch replaces the surface")
}

fn input_binding(nodes: &[PresentationNode]) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
        PresentationNode::Group { children, .. } => input_binding(children),
        _ => None,
    })
}

/// Welcome, then "Create new identity": the name page with its field.
fn on_the_name_page() -> (AppEngine, SurfaceId, u64, BindingId) {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let welcome = engine.initial_commands().expect("welcome");
    let (surface_id, interaction_id) = welcome
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .primary
                .as_ref()
                .map(|primary| (surface_id.clone(), primary.interaction_id.clone())),
            _ => None,
        })
        .expect("welcome offers Create new identity");
    let name_page = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("create new identity");
    let (surface_id, revision, nodes) = shown_surface(&name_page);
    let binding_id = input_binding(&nodes).expect("the name page has its field");
    (engine, surface_id, revision, binding_id)
}

// @internal
#[test]
fn an_invalidation_that_changes_the_page_shows_it_under_a_new_revision() {
    let (mut engine, surface_id, name_revision, _) = on_the_name_page();

    let rebuilt = engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation");

    let (rebuilt_id, rebuilt_revision, nodes) = shown_surface(&rebuilt);
    assert_eq!(rebuilt_id, surface_id);
    assert_eq!(
        input_binding(&nodes),
        None,
        "the rebuilt onboarding starts at Welcome, which has no field"
    );
    assert_eq!(rebuilt_revision, name_revision + 1);
}

// @internal
#[test]
fn a_late_focus_end_for_the_name_field_after_an_invalidation_is_dropped_not_refused() {
    let (mut engine, surface_id, _, binding_id) = on_the_name_page();
    engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation");

    let late = engine.dispatch(Event::InputFocusEnded {
        surface_id,
        binding_id,
    });

    assert_eq!(
        late.map_err(|rejection| rejection.to_string()),
        Ok(Vec::new())
    );
}
