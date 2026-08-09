// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The context bar's navigation and secondary buttons toggle their overlay.
//!
//! Activating either a second time closes the overlay it opened, rather than
//! re-presenting it. Core owns that state: `ContextualSurface` is composed
//! fresh per event and cannot remember what is open, so the engine tracks it
//! and chooses `PresentOverlay` or `DismissOverlay` (ADR-066 — the shell
//! renders the command, it does not decide).

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::{Command, Event};

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

/// The Core-minted interaction id for a context-bar role on the current
/// surface, read from the batch rather than constructed.
fn context_interaction(engine: &mut AppEngine, role: &str) -> (String, String) {
    let commands = engine.initial_commands().expect("initial commands");
    for command in &commands {
        if let Command::SetContextBar {
            surface_id, bar, ..
        } = command
        {
            let spec = match role {
                "navigation" => bar.navigation.as_ref(),
                "secondary" => bar.secondary.as_ref(),
                other => panic!("unknown context-bar role {other}"),
            }
            .unwrap_or_else(|| panic!("context bar must offer a {role} action"));
            return (
                surface_id.as_str().to_owned(),
                spec.interaction_id.as_str().to_owned(),
            );
        }
    }
    panic!("initial batch must set a context bar");
}

fn activate(engine: &mut AppEngine, surface_id: &str, interaction_id: &str) -> Vec<Command> {
    engine
        .dispatch(Event::ActionActivated {
            surface_id: vauchi_core::SurfaceId::new(surface_id).expect("surface id"),
            interaction_id: vauchi_core::InteractionId::new(interaction_id)
                .expect("interaction id"),
        })
        .expect("dispatch activation")
}

fn overlay_commands(commands: &[Command]) -> (usize, usize) {
    let present = commands
        .iter()
        .filter(|c| matches!(c, Command::PresentOverlay { .. }))
        .count();
    let dismiss = commands
        .iter()
        .filter(|c| matches!(c, Command::DismissOverlay { .. }))
        .count();
    (present, dismiss)
}

// @internal
#[test]
fn second_activation_of_the_secondary_button_dismisses_its_overlay() {
    let mut engine = engine_with_identity();
    let (surface_id, interaction_id) = context_interaction(&mut engine, "secondary");

    let opened = activate(&mut engine, &surface_id, &interaction_id);
    assert_eq!(
        overlay_commands(&opened),
        (1, 0),
        "first activation must present the overlay; got {opened:?}"
    );

    let closed = activate(&mut engine, &surface_id, &interaction_id);
    assert_eq!(
        overlay_commands(&closed),
        (0, 1),
        "second activation must dismiss it, not re-present it; got {closed:?}"
    );
}

// @internal
#[test]
fn second_activation_of_the_navigation_button_dismisses_its_overlay() {
    let mut engine = engine_with_identity();
    let (surface_id, interaction_id) = context_interaction(&mut engine, "navigation");

    let opened = activate(&mut engine, &surface_id, &interaction_id);
    assert_eq!(overlay_commands(&opened), (1, 0), "first activation opens");

    let closed = activate(&mut engine, &surface_id, &interaction_id);
    assert_eq!(
        overlay_commands(&closed),
        (0, 1),
        "second activation must dismiss it; got {closed:?}"
    );
}

// @internal
#[test]
fn a_third_activation_reopens_after_the_toggle_closed_it() {
    let mut engine = engine_with_identity();
    let (surface_id, interaction_id) = context_interaction(&mut engine, "secondary");

    activate(&mut engine, &surface_id, &interaction_id);
    activate(&mut engine, &surface_id, &interaction_id);
    let reopened = activate(&mut engine, &surface_id, &interaction_id);

    assert_eq!(
        overlay_commands(&reopened),
        (1, 0),
        "the toggle must return to open, not latch closed; got {reopened:?}"
    );
}

// @internal
#[test]
fn opening_the_other_menu_replaces_rather_than_dismisses() {
    // Navigation open, then secondary tapped: the user asked for the
    // secondary menu, so it must open — never resolve to a dismissal
    // because *an* overlay happened to be open.
    let mut engine = engine_with_identity();
    let (surface_id, navigation_id) = context_interaction(&mut engine, "navigation");
    let (_, secondary_id) = context_interaction(&mut engine, "secondary");

    activate(&mut engine, &surface_id, &navigation_id);
    let switched = activate(&mut engine, &surface_id, &secondary_id);

    let (present, dismiss) = overlay_commands(&switched);
    assert_eq!(
        present, 1,
        "tapping the other menu must present it; got {switched:?}"
    );
    assert_eq!(
        dismiss, 0,
        "and must not emit a dismissal for it; got {switched:?}"
    );
}

// @internal
#[test]
fn a_native_dismissal_clears_the_toggle_state() {
    // The shell reports its own dismissal (tap outside, Close). The next
    // button press must open again, not toggle closed against stale state.
    let mut engine = engine_with_identity();
    let (surface_id, interaction_id) = context_interaction(&mut engine, "secondary");

    activate(&mut engine, &surface_id, &interaction_id);
    engine
        .dispatch(Event::OverlayDismissed {
            surface_id: vauchi_core::SurfaceId::new(&surface_id).expect("surface id"),
            kind: vauchi_core::OverlayKind::ActionMenu,
        })
        .expect("dispatch dismissal");

    let reopened = activate(&mut engine, &surface_id, &interaction_id);
    assert_eq!(
        overlay_commands(&reopened),
        (1, 0),
        "after a native dismissal the button must open again; got {reopened:?}"
    );
}

/// Choosing a destination from the navigation overlay closes it.
///
/// The overlay tracked by `open_overlay` was dismissed only by re-activating
/// the affordance that opened it, or by the shell reporting
/// `OverlayDismissed`. Activating an item *inside* it navigated and left it
/// on screen, covering the destination the user had just chosen. On
/// 2026-08-09 that read on a device as "Settings does not work": the Groups
/// screen was underneath the whole time, still showing the drawer.
///
/// Core owns the dismissal because core owns the overlay state — a shell
/// closing menus on its own would be deciding navigation behaviour.
// @internal
#[test]
fn activating_a_navigation_destination_dismisses_the_overlay() {
    let mut engine = engine_with_identity();
    let (surface_id, nav_interaction) = context_interaction(&mut engine, "navigation");

    let opened = activate(&mut engine, &surface_id, &nav_interaction);
    let items = opened
        .iter()
        .find_map(|command| match command {
            Command::PresentOverlay { overlay, .. } => Some(overlay.items.clone()),
            _ => None,
        })
        .expect("activating the navigation affordance must present the overlay");

    // A destination other than the current screen, so the batch cannot be
    // confused with a no-op re-render of where we already are.
    let destination = items
        .iter()
        .find(|item| item.interaction_id.as_str().ends_with("groups"))
        .expect("navigation overlay must offer Groups");

    let commands = activate(
        &mut engine,
        &surface_id,
        destination.interaction_id.as_str(),
    );

    assert!(
        commands
            .iter()
            .any(|c| matches!(c, Command::DismissOverlay { .. })),
        "choosing a destination must dismiss the navigation overlay; got {commands:#?}"
    );
}

/// Returning to the screen whose menu was open still opens that menu.
///
/// `open_overlay` is keyed by `(surface_id, kind)`, so a stale entry for a
/// screen the user has left could make the next activation there resolve to
/// a dismissal — the menu refusing to open once, for no visible reason.
///
/// This passes with or without the dismissal in `handle_event`; it is a
/// regression guard on the toggle state surviving a round trip, not
/// evidence for that change. It exists because an earlier version of this
/// test only *looked* like it navigated back — `context_interaction`
/// re-reads the bar from whatever surface is current, so it was asserting
/// about Groups while claiming to be home.
// @internal
#[test]
fn the_menu_reopens_after_navigating_away_and_back() {
    fn destination(commands: &[Command], suffix: &str) -> String {
        commands
            .iter()
            .find_map(|command| match command {
                Command::PresentOverlay { overlay, .. } => overlay
                    .items
                    .iter()
                    .find(|item| item.interaction_id.as_str().ends_with(suffix))
                    .map(|item| item.interaction_id.as_str().to_owned()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("navigation overlay must offer `{suffix}`"))
    }

    let mut engine = engine_with_identity();
    let (home_surface, home_nav) = context_interaction(&mut engine, "navigation");
    let opened = activate(&mut engine, &home_surface, &home_nav);
    let groups = destination(&opened, "groups");
    activate(&mut engine, &home_surface, &groups);

    // Now genuinely on Groups: its own bar, its own surface id.
    let (groups_surface, groups_nav) = context_interaction(&mut engine, "navigation");
    assert_ne!(
        groups_surface, home_surface,
        "the engine must actually have moved to another surface"
    );
    let opened_on_groups = activate(&mut engine, &groups_surface, &groups_nav);
    let my_info = destination(&opened_on_groups, "my_info");
    activate(&mut engine, &groups_surface, &my_info);

    // Home again — the affordance must present, not toggle closed.
    let (home_again, nav_again) = context_interaction(&mut engine, "navigation");
    assert_eq!(
        home_again, home_surface,
        "navigation should have returned to the original surface"
    );
    let reopened = activate(&mut engine, &home_again, &nav_again);

    assert!(
        reopened
            .iter()
            .any(|c| matches!(c, Command::PresentOverlay { .. })),
        "the menu must open again after a round trip; got {reopened:#?}"
    );
}
