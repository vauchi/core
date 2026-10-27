// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Drives `AppEngine` the way the shells do: read the emitted `Command`s,
//! find an affordance by its visible label, report the matching `Event`.

use vauchi_app::ui::AppEngine;
use vauchi_core::{ActionSpec, BindingId, Command, Event, PresentationNode, SurfaceId};

pub fn nodes_of(nodes: &[PresentationNode], out: &mut Vec<PresentationNode>) {
    for node in nodes {
        out.push(node.clone());
        if let PresentationNode::Group { children, .. } = node {
            nodes_of(children, out);
        }
    }
}

pub fn surfaces(commands: &[Command]) -> Vec<(SurfaceId, Vec<PresentationNode>)> {
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

pub fn action(commands: &[Command], label: &str) -> Option<(SurfaceId, ActionSpec)> {
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

pub fn activate(
    engine: &mut AppEngine,
    surface_id: SurfaceId,
    action: &ActionSpec,
) -> Vec<Command> {
    engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: action.interaction_id.clone(),
        })
        .expect("activation")
}

pub fn row(commands: &[Command], title: &str) -> (SurfaceId, ActionSpec) {
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

pub fn first_input(commands: &[Command]) -> (SurfaceId, BindingId) {
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

pub fn validation_errors(commands: &[Command]) -> Vec<String> {
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

/// The surface's pending confirmation as (surface, confirm, cancel).
pub fn confirmation(commands: &[Command]) -> Option<(SurfaceId, ActionSpec, ActionSpec)> {
    surfaces(commands)
        .into_iter()
        .find_map(|(surface_id, nodes)| {
            nodes.into_iter().find_map(|node| match node {
                PresentationNode::Confirmation {
                    confirm, cancel, ..
                } => Some((surface_id.clone(), confirm, cancel)),
                _ => None,
            })
        })
}
