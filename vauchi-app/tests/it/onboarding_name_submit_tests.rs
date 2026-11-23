// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The keyboard's Done key on the name page did nothing: a shell reports it
//! as `InputSubmitted`, and onboarding acted on a submit only from the custom
//! group field. On a Pixel 3a the Continue button sits under the keyboard,
//! so the person had to hide the keyboard to go on (vauchi/private#1362).

use super::common::app_engine_helpers::present_the_name_page;
use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::{BindingId, Command, Event, InputValue, PresentationNode, SurfaceId};

struct Shown {
    title: String,
    field: Option<BindingId>,
}

fn shown(commands: &[Command]) -> Shown {
    commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(Shown {
                title: surface.title.clone(),
                field: input(&surface.nodes),
            }),
            _ => None,
        })
        .expect("the batch replaces the surface")
}

fn input(nodes: &[PresentationNode]) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
        PresentationNode::Group { children, .. } => input(children),
        _ => None,
    })
}

fn onboarding() -> SurfaceId {
    SurfaceId::new("onboarding").unwrap()
}

/// The name page with `typed` in its field, and the page's binding.
fn name_page_with(typed: &str) -> (AppEngine, Shown, BindingId) {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let name_page = shown(&present_the_name_page(&mut engine));
    let binding_id = name_page
        .field
        .clone()
        .expect("the name page has its field");
    if !typed.is_empty() {
        engine
            .dispatch(Event::ValueChanged {
                surface_id: onboarding(),
                binding_id: binding_id.clone(),
                value: InputValue::Text(typed.to_owned()),
            })
            .expect("type the name");
    }
    (engine, name_page, binding_id)
}

/// Where Continue leads from the name page with a name typed.
fn page_after_continue() -> String {
    let (mut engine, _, _) = name_page_with("Carol");
    let continue_action = engine
        .initial_commands()
        .expect("re-present")
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar { bar, .. } => bar
                .primary
                .as_ref()
                .map(|primary| primary.interaction_id.clone()),
            _ => None,
        })
        .expect("the name page offers Continue");
    shown(
        &engine
            .dispatch(Event::ActionActivated {
                surface_id: onboarding(),
                interaction_id: continue_action,
            })
            .expect("continue"),
    )
    .title
}

// @internal
#[test]
fn done_on_the_keyboard_with_a_name_typed_goes_where_continue_goes() {
    let (mut engine, name_page, binding_id) = name_page_with("Carol");

    let after = shown(
        &engine
            .dispatch(Event::InputSubmitted {
                surface_id: onboarding(),
                binding_id,
            })
            .expect("submit"),
    );

    assert_ne!(after.title, name_page.title, "the name page was left");
    assert_eq!(after.title, page_after_continue());
}

// @internal
#[test]
fn done_on_the_keyboard_with_no_name_stays_on_the_name_page() {
    let (mut engine, name_page, binding_id) = name_page_with("");

    let after = shown(
        &engine
            .dispatch(Event::InputSubmitted {
                surface_id: onboarding(),
                binding_id: binding_id.clone(),
            })
            .expect("submit"),
    );

    assert_eq!(after.title, name_page.title);
    assert_eq!(after.field, Some(binding_id));
}
