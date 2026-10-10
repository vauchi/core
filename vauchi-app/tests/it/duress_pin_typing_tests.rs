// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A duress PIN is stored as typed, whichever way a shell reports its PIN
//! field (vauchi/private#618). Android sends the field's whole text on each
//! keystroke ("6", "65", "654", …); a shell that echoes Core's masked value
//! sends it back with the new digit ("••5"). Core appended every value, so
//! Android stored `665654` for `654321` and the person's duress PIN was
//! refused under coercion.

use vauchi_app::ui::AppEngine;
use vauchi_core::api::{AuthMode, Vauchi};
use vauchi_core::{
    BindingId, Command, Event, InputValue, PresentationInputKind, PresentationNode, SurfaceId,
};

const PASSWORD: &str = "app-password-123";
const DURESS_PIN: &str = "654321";

/// What a shell sends for its PIN field after each keystroke.
#[derive(Clone, Copy)]
enum Shell {
    /// Keeps its own text and sends all of it (Android).
    OwnText,
    /// Shows Core's masked value and sends it back with the new digit.
    EchoesCore,
}

struct PinField {
    surface_id: SurfaceId,
    binding_id: BindingId,
    value: String,
}

fn pin_field(commands: &[Command]) -> Option<PinField> {
    fn find(nodes: &[PresentationNode]) -> Option<(BindingId, String)> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Input {
                binding_id,
                value,
                input_kind: PresentationInputKind::Pin,
                ..
            } => Some((binding_id.clone(), value.clone())),
            PresentationNode::Group { children, .. } => find(children),
            _ => None,
        })
    }
    commands.iter().find_map(|command| match command {
        Command::ReplaceSurface { surface } => {
            find(&surface.nodes).map(|(binding_id, value)| PinField {
                surface_id: surface.surface_id.clone(),
                binding_id,
                value,
            })
        }
        _ => None,
    })
}

fn activate_next(app: &mut AppEngine, commands: &[Command]) -> Vec<Command> {
    let primary = commands.iter().find_map(|command| match command {
        Command::SetContextBar {
            surface_id, bar, ..
        } => bar
            .primary
            .as_ref()
            .map(|action| (surface_id.clone(), action.interaction_id.clone())),
        _ => None,
    });
    let body = || {
        fn find(nodes: &[PresentationNode]) -> Option<vauchi_core::InteractionId> {
            nodes.iter().find_map(|node| match node {
                PresentationNode::List { rows, .. } => rows
                    .iter()
                    .find_map(|row| row.activation.as_ref().map(|a| a.interaction_id.clone())),
                PresentationNode::Group { children, .. } => find(children),
                _ => None,
            })
        }
        commands.iter().find_map(|command| match command {
            Command::ReplaceSurface { surface } => {
                find(&surface.nodes).map(|id| (surface.surface_id.clone(), id))
            }
            _ => None,
        })
    };
    let (surface_id, interaction_id) = primary.or_else(body).expect("an action to activate");
    app.dispatch(Event::ActionActivated {
        surface_id,
        interaction_id,
    })
    .expect("activate")
}

/// Types `keys` into the PIN field one keystroke at a time; `\u{8}` is a
/// backspace.
fn type_pin(
    app: &mut AppEngine,
    mut commands: Vec<Command>,
    shell: Shell,
    keys: &str,
) -> Vec<Command> {
    let mut own_text = String::new();
    for key in keys.chars() {
        let field = pin_field(&commands).expect("a PIN field on screen");
        let value = match (shell, key) {
            (Shell::OwnText, '\u{8}') => {
                own_text.pop();
                own_text.clone()
            }
            (Shell::OwnText, digit) => {
                own_text.push(digit);
                own_text.clone()
            }
            (Shell::EchoesCore, '\u{8}') => {
                let mut shown = field.value.clone();
                shown.pop();
                shown
            }
            (Shell::EchoesCore, digit) => format!("{}{digit}", field.value),
        };
        commands = app
            .dispatch(Event::ValueChanged {
                surface_id: field.surface_id,
                binding_id: field.binding_id,
                value: InputValue::Text(value),
            })
            .expect("value change");
    }
    commands
}

/// Runs the duress setup the way a shell would, typing the PIN and its
/// confirmation with `keys`, and returns the engine afterwards.
fn set_up_duress_pin(shell: Shell, keys: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.setup_app_password(PASSWORD).expect("app password");
    let mut app = AppEngine::for_duress_setup(vauchi);
    let mut commands = app.initial_commands().expect("initial commands");

    commands = activate_next(&mut app, &commands); // overview → enter PIN
    commands = type_pin(&mut app, commands, shell, keys);
    commands = activate_next(&mut app, &commands); // → confirm PIN
    commands = type_pin(&mut app, commands, shell, keys);
    commands = activate_next(&mut app, &commands); // → alerts
    let commands = activate_next(&mut app, &commands); // save
    assert!(
        commands.contains(&Command::PerformNativeBack),
        "the setup did not complete: {:?}",
        commands
            .iter()
            .map(Command::variant_name)
            .collect::<Vec<_>>()
    );
    app
}

fn unlocks_as(app: &mut AppEngine, pin: &str) -> Option<AuthMode> {
    app.vauchi_mut().authenticate(pin).ok()
}

// @scenario: duress_mode.feature :: Enable duress PIN in settings
#[test]
fn a_shell_sending_its_whole_field_stores_the_pin_as_typed() {
    let mut app = set_up_duress_pin(Shell::OwnText, DURESS_PIN);

    assert_eq!(unlocks_as(&mut app, DURESS_PIN), Some(AuthMode::Duress));
    assert_eq!(unlocks_as(&mut app, "665654"), None);
}

// @scenario: duress_mode.feature :: Enable duress PIN in settings
#[test]
fn a_shell_echoing_cores_masked_value_stores_the_pin_as_typed() {
    let mut app = set_up_duress_pin(Shell::EchoesCore, DURESS_PIN);

    assert_eq!(unlocks_as(&mut app, DURESS_PIN), Some(AuthMode::Duress));
}

// @scenario: duress_mode.feature :: Enable duress PIN in settings
#[test]
fn a_corrected_digit_is_replaced_not_kept() {
    for shell in [Shell::OwnText, Shell::EchoesCore] {
        let mut app = set_up_duress_pin(shell, "6549\u{8}321");

        assert_eq!(unlocks_as(&mut app, DURESS_PIN), Some(AuthMode::Duress));
        assert_eq!(unlocks_as(&mut app, "654932"), None);
    }
}

// @scenario: duress_mode.feature :: Enable duress PIN in settings
#[test]
fn the_pin_field_never_carries_the_digits_back_to_the_shell() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    vauchi.setup_app_password(PASSWORD).expect("app password");
    let mut app = AppEngine::for_duress_setup(vauchi);
    let commands = app.initial_commands().expect("initial commands");
    let commands = activate_next(&mut app, &commands);

    let commands = type_pin(&mut app, commands, Shell::OwnText, "654");

    let field = pin_field(&commands).expect("a PIN field on screen");
    assert_eq!(field.value, "•••");
}
