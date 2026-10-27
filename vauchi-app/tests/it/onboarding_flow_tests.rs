// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Onboarding from first launch to the first screen after it, and the tips
//! banner it leaves behind, driven through the events the shells report.

use crate::event_path_support::{action, activate, first_input, row, surfaces};
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{ActionSpec, Command, Event, InputValue, PresentationNode, SurfaceId};

/// The surface's main call to action, as the context bar offers it.
fn primary(commands: &[Command]) -> (SurfaceId, ActionSpec) {
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar.primary.clone().map(|a| (surface_id.clone(), a)),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no primary action: {commands:#?}"))
}

fn progress(commands: &[Command]) -> Option<f64> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .find_map(|node| match node {
            PresentationNode::Progress { value, .. } => value,
            _ => None,
        })
}

fn title(commands: &[Command]) -> String {
    commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.title.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

/// A fresh install on the name step with "Alice" typed in.
fn named_alice() -> (AppEngine, Vec<Command>) {
    let mut engine = AppEngine::new(Vauchi::in_memory().unwrap());
    let welcome = engine.initial_commands().unwrap();
    let (surface_id, create) = action(&welcome, "Create new identity").expect("create");
    let name = activate(&mut engine, surface_id, &create);
    let (surface_id, binding_id) = first_input(&name);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("Alice".into()),
        })
        .unwrap();
    (engine, typed)
}

fn press_primary(engine: &mut AppEngine, commands: &[Command]) -> Vec<Command> {
    let (surface_id, next) = primary(commands);
    activate(engine, surface_id, &next)
}

/// Name, then Continue past groups, Skip contact info, Start using the app.
fn shortest_onboarding() -> (AppEngine, Vec<Command>) {
    let (mut engine, named) = named_alice();
    let groups = press_primary(&mut engine, &named);
    let info = press_primary(&mut engine, &groups);
    let (surface_id, skip) = row(&info, "Skip");
    let what_next = activate(&mut engine, surface_id, &skip);
    assert_eq!(title(&what_next), "What would you like to do?");
    let landed = press_primary(&mut engine, &what_next);
    (engine, landed)
}

fn own_field_count(engine: &AppEngine) -> usize {
    engine.vauchi().own_card().unwrap().unwrap().fields().len()
}

fn banners(commands: &[Command]) -> Vec<(SurfaceId, ActionSpec)> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(surface_id, nodes)| {
            nodes.into_iter().filter_map(move |node| match node {
                PresentationNode::Status {
                    activation: Some(a),
                    ..
                } if a.label == "Dismiss" => Some((surface_id.clone(), a)),
                _ => None,
            })
        })
        .collect()
}

// @scenario: onboarding :: Onboarding progress indicator
#[test]
fn each_of_the_four_steps_shows_how_far_along_it_is() {
    let (mut engine, named) = named_alice();
    let groups = press_primary(&mut engine, &named);
    let info = press_primary(&mut engine, &groups);
    let what_next = press_primary(&mut engine, &info);

    let shown: Vec<Option<f64>> = [&named, &groups, &info, &what_next]
        .into_iter()
        .map(|step| progress(step))
        .collect();

    assert_eq!(shown, [Some(0.25), Some(0.5), Some(0.75), Some(1.0)]);
}

// @scenario: onboarding :: Can skip optional steps
#[test]
fn skipping_groups_and_contact_info_still_finishes_onboarding() {
    let (engine, _) = shortest_onboarding();

    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
    assert_eq!(own_field_count(&engine), 0);
    assert_eq!(engine.vauchi().list_groups().unwrap().len(), 0);
}

// @scenario: onboarding :: Complete onboarding in under 30 seconds
#[test]
fn the_shortest_path_ends_on_a_card_ready_to_exchange() {
    let (engine, landed) = shortest_onboarding();

    assert_eq!(
        engine.vauchi().own_card().unwrap().unwrap().display_name(),
        "Alice"
    );
    assert_eq!(primary(&landed).1.label, "Exchange Now");
}

// @scenario: onboarding :: Onboarding completion via WhatNext
#[test]
fn starting_the_app_from_what_next_ends_onboarding_for_good() {
    let (engine, _) = shortest_onboarding();

    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
    assert_eq!(engine.default_screen(), AppScreen::MyInfo);
}

// @scenario: onboarding :: Prompt for first exchange
#[test]
fn the_first_screen_invites_an_exchange_that_one_tap_opens() {
    let (mut engine, landed) = shortest_onboarding();
    let invitation: Vec<String> = surfaces(&landed)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::Group {
                label: Some(label),
                children,
                ..
            } if label == "Ready to exchange?" => Some(children),
            _ => None,
        })
        .flatten()
        .filter_map(|node| match node {
            PresentationNode::Status { title, .. } => Some(title),
            _ => None,
        })
        .collect();

    press_primary(&mut engine, &landed);

    assert_eq!(invitation, ["Find someone nearby"]);
    assert_eq!(engine.current_app_screen(), &AppScreen::Exchange);
}

// @scenario: onboarding :: Demo contact is dismissible
#[test]
fn a_dismissed_tips_banner_does_not_come_back() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi.initialize_demo_contact().unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Contacts);
    let contacts = engine.initial_commands().unwrap();
    let [(surface_id, dismiss)] = banners(&contacts).try_into().expect("one tips banner");

    activate(&mut engine, surface_id, &dismiss);
    engine.navigate_to(AppScreen::MyInfo);
    engine.navigate_to(AppScreen::Contacts);
    let revisited = engine.initial_commands().unwrap();

    assert_eq!(banners(&revisited).len(), 0);
    assert!(!engine.vauchi().is_demo_contact_active().unwrap());
}
