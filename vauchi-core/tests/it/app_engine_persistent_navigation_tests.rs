// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `Command::SetNavigation` tests: the persistent navigation surface
//! (ADR-066 Amendment 2026-07-18 D4) tracks the currently selected
//! destination across navigation, and publishes no items while the app
//! is locked.

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::Command;
use vauchi_core::api::Vauchi;

fn navigation_selection(commands: &[Command]) -> Vec<(&str, bool)> {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetNavigation { navigation, .. } => Some(
                navigation
                    .items
                    .iter()
                    .map(|item| (item.interaction_id.as_str(), item.selected))
                    .collect(),
            ),
            _ => None,
        })
        .expect("initial commands must publish SetNavigation")
}

// @internal
#[test]
fn navigating_contacts_then_my_card_moves_the_selected_navigation_item() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Contacts);
    let commands = engine.initial_commands().expect("contacts commands");
    let selection = navigation_selection(&commands);
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && id.ends_with("contacts"))
            .count(),
        1,
        "contacts item must be the sole selection: {selection:?}"
    );

    engine.navigate_to(AppScreen::MyInfo);
    let commands = engine.initial_commands().expect("my card commands");
    let selection = navigation_selection(&commands);
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && id.ends_with("my_info"))
            .count(),
        1,
        "my card item must be the sole selection: {selection:?}"
    );
    assert_eq!(
        selection
            .iter()
            .filter(|(id, selected)| *selected && !id.ends_with("my_info"))
            .count(),
        0,
        "only my card may stay selected after navigating away from contacts: {selection:?}"
    );
}

// @internal
#[test]
fn locked_app_publishes_navigation_with_no_items() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let mut engine = AppEngine::new(vauchi);

    engine.navigate_to(AppScreen::Lock);
    let commands = engine.initial_commands().expect("lock commands");

    assert!(
        navigation_selection(&commands).is_empty(),
        "a locked app must publish no navigation destinations"
    );
}

fn primary_action(commands: &[Command]) -> (vauchi_core::SurfaceId, vauchi_core::InteractionId) {
    commands
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
        .expect("batch must carry a context bar with a primary action")
}

fn first_input_binding(nodes: &[vauchi_core::PresentationNode]) -> Option<vauchi_core::BindingId> {
    nodes.iter().find_map(|node| match node {
        vauchi_core::PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
        vauchi_core::PresentationNode::Group { children, .. } => first_input_binding(children),
        _ => None,
    })
}

/// Every batch that replaces a surface must publish that surface's
/// navigation at the same revision, and after the replacement: a shell
/// keyed on `(surface_id, revision)` drops a `SetNavigation` that arrives
/// first or for a stale revision, and then hides its sidebar or bar.
fn assert_navigation_follows_replacement(commands: &[Command], step: &str) {
    for (replaced_at, command) in commands.iter().enumerate() {
        let Command::ReplaceSurface { surface } = command else {
            continue;
        };
        let navigation_at = commands
            .iter()
            .position(|command| {
                matches!(
                    command,
                    Command::SetNavigation { surface_id, revision, .. }
                        if *surface_id == surface.surface_id && *revision == surface.revision
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "{step}: no SetNavigation for surface {:?} at revision {} in {commands:?}",
                    surface.surface_id, surface.revision
                )
            });
        assert!(
            navigation_at > replaced_at,
            "{step}: SetNavigation must follow ReplaceSurface so a shell keyed on the surface revision keeps it"
        );
    }
}

// @internal
#[test]
fn completing_onboarding_publishes_navigation_for_the_home_surface() {
    let vauchi = Vauchi::in_memory().unwrap();
    let mut engine = AppEngine::new(vauchi);
    let mut commands = engine.initial_commands().expect("onboarding commands");
    assert_navigation_follows_replacement(&commands, "welcome");

    // Welcome -> display name.
    let (surface_id, interaction_id) = primary_action(&commands);
    commands = engine
        .dispatch(vauchi_core::Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("create new identity");
    assert_navigation_follows_replacement(&commands, "display name");

    let (surface_id, binding_id) = commands
        .iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => first_input_binding(&surface.nodes)
                .map(|binding| (surface.surface_id.clone(), binding)),
            _ => None,
        })
        .expect("display-name surface has an input");
    commands = engine
        .dispatch(vauchi_core::Event::ValueChanged {
            surface_id,
            binding_id,
            value: vauchi_core::InputValue::Text("Walker".to_owned()),
        })
        .expect("type the display name");

    // Display name -> groups -> contact info -> what next -> home.
    for step in ["groups", "contact info", "what next", "home"] {
        let (surface_id, interaction_id) = primary_action(&commands);
        commands = engine
            .dispatch(vauchi_core::Event::ActionActivated {
                surface_id,
                interaction_id,
            })
            .expect(step);
        assert_navigation_follows_replacement(&commands, step);
    }

    assert_eq!(engine.current_app_screen(), &AppScreen::MyInfo);
    let items = commands
        .iter()
        .find_map(|command| match command {
            Command::SetNavigation { navigation, .. } => Some(navigation.items.len()),
            _ => None,
        })
        .expect("home batch publishes SetNavigation");
    assert_eq!(
        items, 5,
        "home navigation lists Core's primary destinations"
    );
}
