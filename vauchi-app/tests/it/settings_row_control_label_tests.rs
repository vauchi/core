// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A settings row names its setting once. The row title already carries
//! the name, so a control parked in that row must not repeat it as visible
//! copy.
//!
//! Sending the label twice made the two shells fail in opposite directions
//! on identical commands: Android rendered both and showed every setting
//! name twice, iOS rendered only the title and dropped the control
//! entirely.
//! (`_private/docs/problems/2026-08-20-ios-settings-toggles-render-no-control/`)

use vauchi_app::ui::{PreparedSurface, SettingsConfig, SettingsEngine, WorkflowEngine};
use vauchi_core::{Command, PresentationNode, SurfaceId};

/// The nodes the settings screen projects onto its surface.
fn settings_nodes() -> Vec<PresentationNode> {
    let engine = SettingsEngine::new(SettingsConfig::default());
    let screen = engine.current_screen();
    let surface = PreparedSurface::from_screen(SurfaceId::new("settings").unwrap(), 1, &screen)
        .expect("settings screen projects to a prepared surface");
    match surface.command() {
        Command::ReplaceSurface { surface } => surface.nodes,
        other => panic!("expected ReplaceSurface, got {other:?}"),
    }
}

/// Every `(row title, toggle label)` pair the settings screen projects.
fn toggle_rows() -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    collect(&settings_nodes(), &mut pairs);
    pairs
}

fn collect(nodes: &[PresentationNode], pairs: &mut Vec<(String, String)>) {
    for node in nodes {
        match node {
            PresentationNode::List { rows, .. } => {
                for row in rows {
                    for control in &row.controls {
                        if let PresentationNode::Toggle { label, .. } = control {
                            pairs.push((row.title.clone(), label.clone()));
                        }
                    }
                }
            }
            PresentationNode::Group { children, .. } => collect(children, pairs),
            _ => {}
        }
    }
}

// @internal
#[test]
fn settings_toggle_does_not_repeat_its_row_title() {
    let pairs = toggle_rows();

    assert!(
        !pairs.is_empty(),
        "settings projected no toggle rows, so this test proved nothing"
    );

    for (title, label) in &pairs {
        assert_eq!(
            label, "",
            "row {title:?} carries a toggle labelled {label:?}; the row title \
             already names the setting, so the control must not repeat it"
        );
    }
}

// @internal
#[test]
fn settings_toggle_still_names_its_setting_to_assistive_tech() {
    let mut labels = Vec::new();
    accessibility_labels(&settings_nodes(), &mut labels);

    assert!(
        !labels.is_empty(),
        "settings projected no toggle rows, so this test proved nothing"
    );
    for label in &labels {
        assert!(
            !label.is_empty(),
            "a toggle with no visible label must still carry an accessibility \
             label, or a screen-reader user cannot tell which setting it is"
        );
    }
}

fn accessibility_labels(nodes: &[PresentationNode], labels: &mut Vec<String>) {
    for node in nodes {
        match node {
            PresentationNode::List { rows, .. } => {
                for row in rows {
                    for control in &row.controls {
                        if let PresentationNode::Toggle { accessibility, .. } = control {
                            labels.push(accessibility.label.clone());
                        }
                    }
                }
            }
            PresentationNode::Group { children, .. } => accessibility_labels(children, labels),
            _ => {}
        }
    }
}
