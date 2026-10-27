// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Exchange destination in the navigation bar, tapped the way the
//! shells report it.

use crate::event_path_support::surfaces;
use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event, PresentationNode};

fn activatable_rows(commands: &[Command]) -> Vec<String> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::List { rows, .. } => Some(rows),
            _ => None,
        })
        .flatten()
        .filter(|r| r.activation.is_some())
        .map(|r| r.title)
        .collect()
}

// @scenario: navigation :: Exchange is accessible from navigation
#[test]
fn the_exchange_tab_opens_the_ways_to_exchange() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let my_card = engine.initial_commands().unwrap();
    let (surface_id, interaction_id) = my_card
        .iter()
        .find_map(|command| match command {
            Command::SetNavigation {
                surface_id,
                navigation,
                ..
            } => navigation
                .items
                .iter()
                .find(|item| item.label == "Exchange")
                .map(|item| (surface_id.clone(), item.interaction_id.clone())),
            _ => None,
        })
        .expect("an Exchange destination in the navigation bar");

    let shown = engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .unwrap();

    assert_eq!(engine.current_app_screen(), &AppScreen::Exchange);
    assert_eq!(activatable_rows(&shown), ["Link", "Other ways to connect"]);
}
