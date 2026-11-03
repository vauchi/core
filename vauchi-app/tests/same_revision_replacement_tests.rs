// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A replacement that keeps a surface's revision keeps its structure: the
//! same nodes, in the same order, with the same bindings. Shells rely on it
//! to update what they show in place instead of rebuilding it, which keeps
//! a focused input's caret, selection and composition (vauchi/private#1062).

use vauchi_app::ui::AppEngine;
use vauchi_core::{
    BindingId, Command, Event, InputValue, InteractionId, PresentationNode, SurfaceId, SurfaceSpec,
    api::Vauchi,
};

/// Each node's kind and identity, children included: what a shell matches
/// widgets by. Values, labels and enabled state are left out; those are
/// what an in-place update changes.
fn structure(nodes: &[PresentationNode]) -> Vec<String> {
    fn walk(value: &serde_json::Value, out: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(map) => {
                for (kind, node) in map {
                    let identity = ["binding_id", "id", "interaction_id"]
                        .iter()
                        .find_map(|key| node.get(*key).and_then(|v| v.as_str()))
                        .unwrap_or("");
                    out.push(format!("{kind}:{identity}"));
                    for key in ["children", "rows", "nodes"] {
                        if let Some(children) = node.get(key) {
                            walk(children, out);
                        }
                    }
                }
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(
        &serde_json::to_value(nodes).expect("nodes serialize"),
        &mut out,
    );
    out
}

fn replaced_surface(commands: &[Command]) -> Option<&SurfaceSpec> {
    commands.iter().find_map(|command| match command {
        Command::ReplaceSurface { surface } => Some(surface),
        _ => None,
    })
}

fn primary_action(commands: &[Command]) -> (SurfaceId, InteractionId) {
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
        .expect("onboarding has a primary action")
}

fn first_input(surface: &SurfaceSpec) -> BindingId {
    surface
        .nodes
        .iter()
        .find_map(|node| match node {
            PresentationNode::Input { binding_id, .. } => Some(binding_id.clone()),
            _ => None,
        })
        .expect("the name step has an input")
}

// @scenario: generic_presentation_protocol.feature :: Responsive transitions preserve interaction state
#[test]
fn typing_keeps_the_revision_and_the_structure_of_the_surface() {
    let mut app = AppEngine::new(Vauchi::in_memory().expect("in-memory core"));
    let welcome = app.initial_commands().expect("initial commands");
    let welcome_surface = replaced_surface(&welcome).expect("welcome surface").clone();
    let (surface_id, create) = primary_action(&welcome);
    let name_step = app
        .dispatch(Event::ActionActivated {
            surface_id: surface_id.clone(),
            interaction_id: create,
        })
        .expect("create new identity");
    let shown = replaced_surface(&name_step)
        .expect("name step surface")
        .clone();
    let binding_id = first_input(&shown);

    // A new revision is a new structure: the comparison below can tell.
    assert_ne!(shown.revision, welcome_surface.revision);
    assert_ne!(structure(&shown.nodes), structure(&welcome_surface.nodes));

    let long_name = "N".repeat(400);
    for typed in ["A", "Al", "Ali", "", "   ", long_name.as_str(), "Alice"] {
        let answer = app
            .dispatch(Event::ValueChanged {
                surface_id: surface_id.clone(),
                binding_id: binding_id.clone(),
                value: InputValue::Text(typed.to_string()),
            })
            .expect("value accepted");
        let Some(replaced) = replaced_surface(&answer) else {
            continue;
        };
        if replaced.revision == shown.revision {
            assert_eq!(
                structure(&replaced.nodes),
                structure(&shown.nodes),
                "typing {:?} kept revision {} but changed the structure",
                typed.chars().take(12).collect::<String>(),
                shown.revision
            );
        }
    }
}
