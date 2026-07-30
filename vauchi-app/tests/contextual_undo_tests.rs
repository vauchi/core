// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_app::ui::ContextualActionCoordinator;
use vauchi_core::{
    ActionSpec, ActionTone, Command, ContextBar, Event, InteractionId, StandardShortcut, SurfaceId,
};

fn interaction(id: &str) -> InteractionId {
    InteractionId::new(id).expect("valid interaction id")
}

fn action(id: &str, label: &str, shortcut: Option<StandardShortcut>) -> ActionSpec {
    ActionSpec {
        interaction_id: interaction(id),
        label: label.to_owned(),
        accessibility_label: label.to_owned(),
        icon_token: None,
        enabled: true,
        tone: ActionTone::Standard,
        shortcut,
    }
}

fn initial_bar() -> ContextBar {
    ContextBar {
        back: Some(action("back", "Back", Some(StandardShortcut::Back))),
        navigation: Some(action("navigate", "Navigate", None)),
        primary: Some(action(
            "save",
            "Save",
            Some(StandardShortcut::ActivatePrimary),
        )),
        secondary: Some(action("more", "More actions", None)),
    }
}

fn set_bar(bar: ContextBar) -> Command {
    Command::SetContextBar {
        surface_id: SurfaceId::new("detail").expect("valid surface id"),
        revision: 1,
        bar: Box::new(bar),
    }
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Primary action becomes causal Undo
// @scenario: generic_presentation_protocol.feature :: Primary action becomes causal Undo
#[test]
fn test_causal_undo_replaces_and_restores_the_same_primary_action() {
    let original = initial_bar();
    let mut coordinator = ContextualActionCoordinator::new(
        SurfaceId::new("detail").expect("valid surface id"),
        1,
        original.clone(),
    );
    let undo = action("undo_save", "Undo", Some(StandardShortcut::Undo));
    let mut undo_bar = original.clone();
    undo_bar.primary = Some(undo.clone());

    assert_eq!(
        coordinator
            .offer_causal_undo(&interaction("save"), undo)
            .expect("primary caused the reversible mutation"),
        vec![set_bar(undo_bar)]
    );

    let transition = coordinator
        .handle_event(Event::ActionActivated {
            surface_id: SurfaceId::new("detail").expect("valid surface id"),
            interaction_id: interaction("undo_save"),
        })
        .expect("undo activation");
    assert_eq!(transition.interaction_id, interaction("undo_save"));
    assert!(transition.undo_requested);
    assert_eq!(transition.commands, vec![set_bar(original)]);
}

/// Feature: generic_presentation_protocol.feature
/// Scenario: Primary action becomes causal Undo
// @scenario: generic_presentation_protocol.feature :: Primary action becomes causal Undo
#[test]
fn test_secondary_cause_replaces_the_primary_with_causal_undo() {
    let original = initial_bar();
    let mut coordinator = ContextualActionCoordinator::new(
        SurfaceId::new("detail").expect("valid surface id"),
        1,
        original.clone(),
    );
    let undo = action("undo_more", "Undo", Some(StandardShortcut::Undo));
    let mut undo_bar = original;
    undo_bar.primary = Some(undo.clone());

    assert_eq!(
        coordinator
            .offer_causal_undo(&interaction("more"), undo)
            .expect("the reducer result proves the secondary action caused the mutation"),
        vec![set_bar(undo_bar)]
    );
}

// @scenario: generic_presentation_protocol.feature :: Primary action becomes causal Undo
#[test]
fn causal_undo_rebases_onto_the_next_atomic_surface_revision() {
    let mut coordinator = ContextualActionCoordinator::new(
        SurfaceId::new("detail").expect("valid surface id"),
        1,
        initial_bar(),
    );
    coordinator
        .offer_causal_undo_routed(
            &interaction("save"),
            action(
                "surface.2.context.undo",
                "Undo",
                Some(StandardShortcut::Undo),
            ),
            "undo_save".into(),
        )
        .expect("causal undo");

    let mut revision_two = initial_bar();
    revision_two.primary = Some(action(
        "surface.2.context.save",
        "Save",
        Some(StandardShortcut::ActivatePrimary),
    ));
    coordinator
        .rebase(2, revision_two, interaction("surface.2.context.undo"))
        .expect("rebase pending undo");

    let transition = coordinator
        .handle_event(Event::ActionActivated {
            surface_id: SurfaceId::new("detail").unwrap(),
            interaction_id: interaction("surface.2.context.undo"),
        })
        .expect("current undo");
    assert!(transition.undo_requested);
    assert_eq!(transition.action_id.as_deref(), Some("undo_save"));
}
