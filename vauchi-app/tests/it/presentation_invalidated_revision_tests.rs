// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A shell sends `PresentationInvalidated` when content arrives after
//! start, such as the locale catalogue on a fresh install. Core used to
//! rebuild onboarding, which starts again at Welcome, and sent Welcome at
//! the name page's revision: a focus end for the name field still on its
//! way read as current and was refused as an unknown binding, and the
//! person saw "Something went wrong" on Welcome (vauchi/private#1339,
//! Pixel 3a, 2026-11-22). Onboarding now survives the rebuild
//! (vauchi/private#821), so the page, its field and its revision stay.

use super::common::app_engine_helpers::present_the_name_page;
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
    let name_page = present_the_name_page(&mut engine);
    let (surface_id, revision, nodes) = shown_surface(&name_page);
    let binding_id = input_binding(&nodes).expect("the name page has its field");
    (engine, surface_id, revision, binding_id)
}

// @internal
#[test]
fn an_invalidation_on_the_name_page_keeps_the_page_and_its_revision() {
    let (mut engine, surface_id, name_revision, binding_id) = on_the_name_page();

    let rebuilt = engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation");

    let (rebuilt_id, rebuilt_revision, nodes) = shown_surface(&rebuilt);
    assert_eq!(rebuilt_id, surface_id);
    assert_eq!(input_binding(&nodes), Some(binding_id));
    assert_eq!(
        rebuilt_revision, name_revision,
        "fields that keep their meaning keep their revision, so the keyboard stays"
    );
}

// @internal
#[test]
fn a_late_focus_end_for_the_name_field_after_an_invalidation_is_accepted() {
    let (mut engine, surface_id, _, binding_id) = on_the_name_page();
    engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation");

    let late = engine.dispatch(Event::InputFocusEnded {
        surface_id,
        binding_id: binding_id.clone(),
    });

    assert_eq!(
        late.map(|_| ()).map_err(|rejection| rejection.to_string()),
        Ok(())
    );
    let (_, _, nodes) = shown_surface(&engine.initial_commands().expect("re-present"));
    assert_eq!(input_binding(&nodes), Some(binding_id));
}

// On the phone the rebuild came before the invalidation event: locale
// content applied in the background is absorbed at the start of the next
// dispatch (Pixel 3a, 2026-11-22 23:17, core cec092e62).
// @internal
#[test]
fn a_late_focus_end_after_locales_reloaded_is_accepted() {
    let (mut engine, surface_id, name_revision, binding_id) = on_the_name_page();
    engine.reload_locales();

    let rebuilt = engine
        .dispatch(Event::PresentationInvalidated)
        .expect("invalidation");
    let late = engine.dispatch(Event::InputFocusEnded {
        surface_id,
        binding_id: binding_id.clone(),
    });

    let (_, rebuilt_revision, nodes) = shown_surface(&rebuilt);
    assert_eq!(input_binding(&nodes), Some(binding_id));
    assert_eq!(rebuilt_revision, name_revision);
    assert_eq!(
        late.map(|_| ()).map_err(|rejection| rejection.to_string()),
        Ok(())
    );
}
