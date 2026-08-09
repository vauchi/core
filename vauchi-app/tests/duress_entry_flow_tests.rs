// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core-owned duress-setup entry flow (ADR-069).
//!
//! A Rust-native shell must reach duress setup without importing
//! `AppScreen` and without calling a domain mutation directly. These
//! tests pin the flow through the generic reducer boundary only —
//! `initial_commands` / `dispatch(Event)` — because that is the surface
//! `cli`'s `run_with_io` loop consumes.

use vauchi_app::ui::AppEngine;
use vauchi_core::{
    BindingId, Command, Event, InputValue, PresentationNode, SurfaceId, api::Vauchi,
};

fn identity_core() -> Vauchi {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    vauchi
}

fn surface_named<'a>(commands: &'a [Command], name: &str) -> Option<&'a vauchi_core::SurfaceSpec> {
    commands.iter().find_map(|command| match command {
        Command::ReplaceSurface { surface } if surface.surface_id.as_str() == name => Some(surface),
        _ => None,
    })
}

fn input_binding(nodes: &[PresentationNode], label_fragment: &str) -> Option<BindingId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input {
            binding_id, label, ..
        } if label.to_lowercase().contains(label_fragment) => Some(binding_id.clone()),
        PresentationNode::Group { children, .. } => input_binding(children, label_fragment),
        _ => None,
    })
}

fn all_input_bindings(nodes: &[PresentationNode]) -> Vec<BindingId> {
    let mut found = Vec::new();
    for node in nodes {
        match node {
            PresentationNode::Input { binding_id, .. } => found.push(binding_id.clone()),
            PresentationNode::Group { children, .. } => found.extend(all_input_bindings(children)),
            _ => {}
        }
    }
    found
}

fn primary_interaction(commands: &[Command]) -> (SurfaceId, vauchi_core::InteractionId) {
    commands
        .iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .primary
                .as_ref()
                .map(|action| (surface_id.clone(), action.interaction_id.clone())),
            _ => None,
        })
        .expect("a primary context-bar action")
}

fn fill(app: &mut AppEngine, commands: &[Command], surface: &str, value: &str) -> Vec<Command> {
    let surface = surface_named(commands, surface).expect("expected surface");
    let surface_id = surface.surface_id.clone();
    let bindings = all_input_bindings(&surface.nodes);
    assert!(
        !bindings.is_empty(),
        "expected at least one input on {surface_id:?}"
    );
    let mut latest = Vec::new();
    for binding_id in bindings {
        latest = app
            .dispatch(Event::ValueChanged {
                surface_id: surface_id.clone(),
                binding_id,
                value: InputValue::Text(value.to_string()),
            })
            .expect("value change");
    }
    latest
}

/// Walk a multi-step engine sub-flow the way a shell's reducer loop does:
/// populate whatever inputs the current surface exposes, then activate the
/// primary action, until the flow hands control back.
///
/// `DuressPinEngine` is a four-step sub-flow (overview → enter PIN → confirm
/// → alerts), so no single dispatch completes it.
fn drive_until_native_back(
    app: &mut AppEngine,
    mut commands: Vec<Command>,
    value: &str,
    max_steps: usize,
) -> Vec<Command> {
    for _ in 0..max_steps {
        if commands.contains(&Command::PerformNativeBack) {
            return commands;
        }
        if let Some(surface) = commands.iter().find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        }) {
            let surface_id = surface.surface_id.clone();
            for binding_id in all_input_bindings(&surface.nodes) {
                commands = app
                    .dispatch(Event::ValueChanged {
                        surface_id: surface_id.clone(),
                        binding_id,
                        value: InputValue::Text(value.to_string()),
                    })
                    .expect("value change");
            }
        }
        let Some((surface_id, interaction_id)) = optional_primary_interaction(&commands) else {
            return commands;
        };
        commands = app
            .dispatch(Event::ActionActivated {
                surface_id,
                interaction_id,
            })
            .expect("activate primary");
    }
    panic!("duress sub-flow did not terminate within {max_steps} steps");
}

fn optional_primary_interaction(
    commands: &[Command],
) -> Option<(SurfaceId, vauchi_core::InteractionId)> {
    commands.iter().find_map(|command| match command {
        Command::SetContextBar {
            surface_id, bar, ..
        } => bar
            .primary
            .as_ref()
            .map(|action| (surface_id.clone(), action.interaction_id.clone())),
        _ => None,
    })
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn duress_entry_without_an_app_password_opens_password_setup_first() {
    let mut app = AppEngine::for_duress_setup(identity_core());

    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");

    assert!(
        surface_named(&commands, "change_password").is_some(),
        "core enforces app-password-before-duress-PIN (VauchiError::InvalidState), so the entry \
         flow must open password setup first, got {:?}",
        commands
            .iter()
            .filter_map(|c| match c {
                Command::ReplaceSurface { surface } =>
                    Some(surface.surface_id.as_str().to_string()),
                _ => None,
            })
            .collect::<Vec<_>>()
    );
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn duress_entry_with_an_app_password_opens_the_duress_pin_surface_directly() {
    let mut vauchi = identity_core();
    vauchi
        .setup_app_password("existing-pin-1234")
        .expect("app password");
    let mut app = AppEngine::for_duress_setup(vauchi);

    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");

    assert!(
        surface_named(&commands, "duress_pin").is_some(),
        "with a password already configured the flow must skip straight to duress PIN"
    );
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn completing_password_setup_chains_into_the_duress_pin_surface() {
    let mut app = AppEngine::for_duress_setup(identity_core());
    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");

    let after_input = fill(&mut app, &commands, "change_password", "first-pin-1234");
    let (surface_id, interaction_id) = primary_interaction(&after_input);
    let after_submit = app
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id,
        })
        .expect("submit password setup");

    assert!(
        surface_named(&after_submit, "duress_pin").is_some(),
        "password setup must chain into duress PIN rather than falling back to the navigation \
         root — the shell entered this reducer to configure duress"
    );
    assert!(
        app.vauchi().is_password_enabled().expect("password state"),
        "the app password must be persisted by the chained step"
    );
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn completing_duress_setup_terminates_the_entry_flow() {
    let mut vauchi = identity_core();
    vauchi
        .setup_app_password("existing-pin-1234")
        .expect("app password");
    let mut app = AppEngine::for_duress_setup(vauchi);
    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");

    let after_submit = drive_until_native_back(&mut app, commands, "987654", 12);

    assert!(
        after_submit.contains(&Command::PerformNativeBack),
        "a single-purpose entry flow must hand control back when its task completes; without \
         PerformNativeBack the shell's reducer loop never exits, got {:?}",
        after_submit
            .iter()
            .map(|c| c.variant_name())
            .collect::<Vec<_>>()
    );
    assert!(
        app.vauchi().is_duress_enabled().expect("duress state"),
        "the duress PIN must be persisted before the flow terminates"
    );
}

// @scenario: generic_presentation_protocol.feature :: Every shell renders the same prepared presentation
#[test]
fn a_filled_password_input_reports_as_filled_to_the_shell() {
    let mut app = AppEngine::for_duress_setup(identity_core());
    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");
    let surface = surface_named(&commands, "change_password").expect("password setup surface");
    let surface_id = surface.surface_id.clone();
    let binding = all_input_bindings(&surface.nodes)
        .into_iter()
        .next()
        .expect("a password input");

    let after = app
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id: binding.clone(),
            value: InputValue::Text("first-pin-1234".into()),
        })
        .expect("value change");

    let surface = surface_named(&after, "change_password").expect("password surface re-rendered");
    let value = find_input_value(&surface.nodes, &binding).expect("the input is still presented");
    assert!(
        !value.is_empty(),
        "a secret input must still report that it holds a value; a permanently empty `value` \
         leaves a shell unable to tell 'needs input' from 'already answered', and the CLI's \
         prompt loop re-offers the field forever"
    );
    assert!(
        !value.contains("first-pin-1234"),
        "the masked echo must not carry the secret itself, got {value:?}"
    );
}

fn find_input_value(nodes: &[PresentationNode], wanted: &BindingId) -> Option<String> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::Input {
            binding_id, value, ..
        } if binding_id == wanted => Some(value.clone()),
        PresentationNode::Group { children, .. } => find_input_value(children, wanted),
        _ => None,
    })
}

// @scenario: generic_presentation_protocol.feature :: Release contains only the generic action system
#[test]
fn duress_entry_flow_never_requires_the_shell_to_name_a_screen() {
    let mut app = AppEngine::for_duress_setup(identity_core());

    let commands = app
        .initial_commands()
        .expect("initial duress-entry commands");

    let (_, interaction_id) = primary_interaction(&commands);
    assert!(
        !interaction_id.as_str().is_empty(),
        "the entry flow must expose a Core-minted interaction id so the shell activates it \
         without constructing a navigation target"
    );
    let binding = surface_named(&commands, "change_password")
        .and_then(|surface| input_binding(&surface.nodes, "password"));
    assert!(
        binding.is_some(),
        "the shell must be able to bind input by Core-minted binding id alone"
    );
}
