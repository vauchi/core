// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The app password is stored as typed (vauchi/private#621). The form sent
//! its fields back as one `•` per character and stored whatever value
//! returned, so a shell that shows Core's value and types onto it stored
//! `••••1234` for `pass1234`. Like the lock screen, the form now echoes the
//! value itself; every shell masks password inputs (#619).

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::{
    BindingId, Command, Event, InputValue, PresentationInputKind, PresentationNode, SurfaceId,
};

const PASSWORD: &str = "pass1234";

#[derive(Clone, Copy)]
enum Shell {
    /// Keeps its own text and sends all of it (Android between surfaces).
    OwnText,
    /// Shows Core's value and sends it back with the new character.
    EchoesCore,
}

fn password_fields(commands: &[Command]) -> Vec<(SurfaceId, BindingId, String)> {
    fn collect(
        surface: &SurfaceId,
        nodes: &[PresentationNode],
        out: &mut Vec<(SurfaceId, BindingId, String)>,
    ) {
        for node in nodes {
            match node {
                PresentationNode::Input {
                    binding_id,
                    value,
                    input_kind: PresentationInputKind::Password,
                    ..
                } => out.push((surface.clone(), binding_id.clone(), value.clone())),
                PresentationNode::Group { children, .. } => collect(surface, children, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for command in commands {
        if let Command::ReplaceSurface { surface } = command {
            collect(&surface.surface_id, &surface.nodes, &mut out);
        }
    }
    out
}

fn type_into(
    app: &mut AppEngine,
    mut commands: Vec<Command>,
    field: usize,
    shell: Shell,
    text: &str,
) -> Vec<Command> {
    let mut own_text = String::new();
    for ch in text.chars() {
        let (surface_id, binding_id, shown) = password_fields(&commands)
            .into_iter()
            .nth(field)
            .expect("the password field on screen");
        own_text.push(ch);
        let value = match shell {
            Shell::OwnText => own_text.clone(),
            Shell::EchoesCore => format!("{shown}{ch}"),
        };
        commands = app
            .dispatch(Event::ValueChanged {
                surface_id,
                binding_id,
                value: InputValue::Text(value),
            })
            .expect("value change");
    }
    commands
}

fn submit(app: &mut AppEngine, commands: &[Command]) -> Vec<Command> {
    let (surface_id, interaction_id) = commands
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
        .expect("a primary action");
    app.dispatch(Event::ActionActivated {
        surface_id,
        interaction_id,
    })
    .expect("submit")
}

fn set_up_password(shell: Shell) -> (AppEngine, Vec<Command>) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut app = AppEngine::for_duress_setup(vauchi);
    let commands = app.initial_commands().expect("initial commands");
    assert_eq!(
        password_fields(&commands).len(),
        2,
        "new and confirm fields"
    );

    let commands = type_into(&mut app, commands, 0, shell, PASSWORD);
    let commands = type_into(&mut app, commands, 1, shell, PASSWORD);
    let after_submit = submit(&mut app, &commands);
    (app, after_submit)
}

// @scenario: duress_mode.feature :: Enable duress password (requires app password)
#[test]
fn a_shell_typing_onto_cores_value_stores_the_password_as_typed() {
    let (mut app, _) = set_up_password(Shell::EchoesCore);

    assert!(app.vauchi().is_password_enabled().unwrap());
    assert!(app.vauchi_mut().authenticate(PASSWORD).is_ok());
}

// @scenario: duress_mode.feature :: Enable duress password (requires app password)
#[test]
fn a_shell_sending_its_own_text_stores_the_password_as_typed() {
    let (mut app, _) = set_up_password(Shell::OwnText);

    assert!(app.vauchi_mut().authenticate(PASSWORD).is_ok());
}

// @scenario: duress_mode.feature :: Enable duress password (requires app password)
#[test]
fn the_form_echoes_the_typed_value_as_a_password_input() {
    let mut vauchi = Vauchi::in_memory().expect("in-memory core");
    vauchi.create_identity("Alice").expect("identity");
    let mut app = AppEngine::for_duress_setup(vauchi);
    let commands = app.initial_commands().expect("initial commands");

    let commands = type_into(&mut app, commands, 0, Shell::OwnText, "pa•s");

    let (_, _, shown) = password_fields(&commands)
        .into_iter()
        .next()
        .expect("field");
    assert_eq!(
        shown, "pa•s",
        "a typed • is part of the password, not a mask"
    );
}
