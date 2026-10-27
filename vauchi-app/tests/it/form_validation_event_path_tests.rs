// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A refused field value must reach the shell as the input's validation
//! error through the generic command/event protocol the shells use, not
//! only through the legacy `handle_action` path.

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;
use vauchi_core::{ActionSpec, BindingId, Command, Event, InputValue, PresentationNode, SurfaceId};

fn nodes_of(nodes: &[PresentationNode], out: &mut Vec<PresentationNode>) {
    for node in nodes {
        out.push(node.clone());
        if let PresentationNode::Group { children, .. } = node {
            nodes_of(children, out);
        }
    }
}

fn surfaces(commands: &[Command]) -> Vec<(SurfaceId, Vec<PresentationNode>)> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::ReplaceSurface { surface } => {
                let mut all = Vec::new();
                nodes_of(&surface.nodes, &mut all);
                Some((surface.surface_id.clone(), all))
            }
            _ => None,
        })
        .collect()
}

fn action(commands: &[Command], label: &str) -> Option<(SurfaceId, ActionSpec)> {
    commands.iter().rev().find_map(|command| match command {
        Command::SetContextBar {
            surface_id, bar, ..
        } => [&bar.primary, &bar.secondary, &bar.navigation, &bar.back]
            .into_iter()
            .flatten()
            .find(|a| a.label == label)
            .map(|a| (surface_id.clone(), a.clone())),
        Command::PresentOverlay {
            surface_id,
            overlay,
            ..
        } => overlay
            .items
            .iter()
            .find(|a| a.label == label)
            .map(|a| (surface_id.clone(), a.clone())),
        _ => None,
    })
}

fn activate(engine: &mut AppEngine, surface_id: SurfaceId, action: &ActionSpec) -> Vec<Command> {
    engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: action.interaction_id.clone(),
        })
        .expect("activation")
}

fn row(commands: &[Command], title: &str) -> (SurfaceId, ActionSpec) {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.iter().find_map(|node| match node {
                PresentationNode::List { rows, .. } => rows
                    .iter()
                    .find(|r| r.title == title)
                    .and_then(|r| r.activation.clone())
                    .map(|a| (surface_id.clone(), a)),
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("no row {title}: {commands:#?}"))
}

fn first_input(commands: &[Command]) -> (SurfaceId, BindingId) {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.iter().find_map(|node| match node {
                PresentationNode::Input { binding_id, .. } => {
                    Some((surface_id.clone(), binding_id.clone()))
                }
                _ => None,
            })
        })
        .unwrap_or_else(|| panic!("no input: {commands:#?}"))
}

fn validation_errors(commands: &[Command]) -> Vec<String> {
    surfaces(commands)
        .into_iter()
        .flat_map(|(_, nodes)| nodes)
        .filter_map(|node| match node {
            PresentationNode::Input {
                validation_error, ..
            } => validation_error,
            _ => None,
        })
        .collect()
}

// @scenario: contact_card_management :: Email validation
#[test]
fn an_invalid_email_on_the_add_entry_form_reaches_the_shell_as_the_input_error() {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    vauchi
        .add_own_field(vauchi_core::contact_card::ContactField::new(
            vauchi_core::contact_card::FieldType::Email,
            "Work",
            "alice@example.com",
            0,
        ))
        .unwrap();
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::MyInfo);
    let commands = engine.initial_commands().unwrap();
    let (surface_id, more) = action(&commands, "Actions").expect("Actions");
    let menu = activate(&mut engine, surface_id, &more);
    let (surface_id, add) = action(&menu, "Add Entry").expect("Add Entry");
    let commands = activate(&mut engine, surface_id, &add);
    let (surface_id, email) = row(&commands, "Email");
    let form = activate(&mut engine, surface_id, &email);
    let (surface_id, binding_id) = first_input(&form);
    let typed = engine
        .dispatch(Event::ValueChanged {
            surface_id,
            binding_id,
            value: InputValue::Text("notanemail".into()),
        })
        .unwrap();
    let (surface_id, save) = action(&typed, "Save")
        .or_else(|| action(&form, "Save"))
        .expect("Save");

    let saved = activate(&mut engine, surface_id, &save);

    assert_eq!(
        validation_errors(&saved),
        vec!["Please enter a valid email address".to_string()],
        "{saved:#?}"
    );
}
