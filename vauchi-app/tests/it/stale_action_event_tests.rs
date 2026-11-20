// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A person taps what the shell last rendered, and Core may have rendered
//! the surface again before the tap is reduced: a heartbeat that ran a
//! sync for five seconds held two taps back and re-presented the surface
//! under them, and each tap was answered with the generic error alert
//! (Pixel 3a, vauchi/private#1306). Such a tap is stale, not malformed:
//! Core drops it and logs it, as it does a stale value
//! (`stale_binding_event_tests`). An id Core never minted still fails
//! closed (ADR-066).

#![cfg(feature = "testing")]

use vauchi_app::ui::{AppEngine, AppPresentationError, AppScreen, ContextualSurfaceError};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::{Command, ContextBar, InteractionId, SurfaceId, SurfaceSpec};

struct Shown {
    surface: SurfaceSpec,
    bar: ContextBar,
}

fn on_glance() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Bob").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.set_device_capabilities(DeviceCapabilities {
        has_ble: true,
        has_camera: true,
        ..Default::default()
    });
    let _ = engine.navigate_to(AppScreen::Exchange);
    let _ = engine.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Glance,
    });
    engine
}

fn shown(engine: &mut AppEngine) -> Shown {
    let commands = engine.initial_commands().expect("commands");
    let surface = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.clone()),
            _ => None,
        })
        .expect("a surface to render");
    let bar = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::SetContextBar { bar, .. } => Some((**bar).clone()),
            _ => None,
        })
        .expect("a context bar");
    Shown { surface, bar }
}

/// A BLE discovery re-renders the Glance surface at its next revision.
fn re_rendered_by_a_discovery(engine: &mut AppEngine, before: &Shown) -> Shown {
    engine
        .dispatch(Event::BleDeviceDiscovered {
            id: "stranger".into(),
            rssi: -60,
            adv_data: vec![0xAB, 0xCD],
        })
        .expect("discovery handled");
    let after = shown(engine);
    assert!(
        after.surface.revision > before.surface.revision,
        "precondition: the discovery re-rendered the surface"
    );
    assert_eq!(
        after.surface.surface_id, before.surface.surface_id,
        "precondition: the same surface is still shown"
    );
    after
}

fn tap(surface_id: &SurfaceId, interaction_id: InteractionId) -> Event {
    Event::ActionActivated {
        surface_id: surface_id.clone(),
        interaction_id,
    }
}

fn body_interaction(revision: u64, index: u32) -> InteractionId {
    InteractionId::new(format!("surface.{revision}.interaction.{index}")).expect("interaction id")
}

// @scenario: generic_presentation_protocol :: Invalid boundary input fails safely
#[test]
fn a_tap_on_the_context_bar_of_an_earlier_revision_is_dropped() {
    let mut bob = on_glance();
    let before = shown(&mut bob);
    let back = before
        .bar
        .back
        .clone()
        .expect("the exchange screen offers a way back")
        .interaction_id;
    let after = re_rendered_by_a_discovery(&mut bob, &before);

    let out = bob.dispatch(tap(&before.surface.surface_id, back));

    assert_eq!(
        out,
        Ok(Vec::new()),
        "a tap overtaken by a re-render is dropped, with no alert and no navigation"
    );
    assert_eq!(
        shown(&mut bob).surface.surface_id,
        after.surface.surface_id,
        "the stale back tap must not leave the screen"
    );
}

// @scenario: generic_presentation_protocol :: Invalid boundary input fails safely
#[test]
fn a_tap_on_a_body_control_of_an_earlier_revision_is_dropped() {
    let mut bob = on_glance();
    let before = shown(&mut bob);
    let after = re_rendered_by_a_discovery(&mut bob, &before);

    let out = bob.dispatch(tap(
        &before.surface.surface_id,
        body_interaction(before.surface.revision, 0),
    ));

    assert_eq!(out, Ok(Vec::new()));
    assert_eq!(shown(&mut bob).surface.surface_id, after.surface.surface_id);
}

// @scenario: generic_presentation_protocol :: Invalid boundary input fails safely
#[test]
fn a_tap_on_an_unminted_control_of_the_current_revision_still_fails() {
    let mut bob = on_glance();
    let now = shown(&mut bob);
    let unminted = body_interaction(now.surface.revision, 99_999);

    let out = bob.dispatch(tap(&now.surface.surface_id, unminted.clone()));

    assert_eq!(
        out,
        Err(AppPresentationError::Contextual(
            ContextualSurfaceError::UnknownInteraction(unminted.as_str().to_owned())
        )),
        "an id Core never minted must fail closed"
    );
}

// @scenario: generic_presentation_protocol :: Invalid boundary input fails safely
#[test]
fn a_tap_from_a_future_revision_still_fails() {
    let mut bob = on_glance();
    let now = shown(&mut bob);
    let future = body_interaction(999_999, 0);

    let out = bob.dispatch(tap(&now.surface.surface_id, future.clone()));

    assert_eq!(
        out,
        Err(AppPresentationError::Contextual(
            ContextualSurfaceError::UnknownInteraction(future.as_str().to_owned())
        )),
        "an id newer than the surface cannot be stale"
    );
}

// @scenario: generic_presentation_protocol :: Invalid boundary input fails safely
#[test]
fn a_tap_with_an_id_that_names_no_revision_still_fails() {
    let mut bob = on_glance();
    let now = shown(&mut bob);
    let unscoped = InteractionId::new("go_somewhere").expect("interaction id");

    let out = bob.dispatch(tap(&now.surface.surface_id, unscoped.clone()));

    assert_eq!(
        out,
        Err(AppPresentationError::Contextual(
            ContextualSurfaceError::UnknownInteraction(unscoped.as_str().to_owned())
        ))
    );
}
