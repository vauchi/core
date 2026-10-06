// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Cancel on the Hover screen, tapped the way a shell reports it: the
//! row's Core-minted interaction on the presented surface. The owner
//! reported Cancel doing nothing on the iPhone (2026-10-06).

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::platform::{Command, InteractionId, PresentationNode, SurfaceSpec};

fn engine_on_hover() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory Vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::MultiStageExchange { .. }
    ));
    engine
}

fn presented(engine: &mut AppEngine) -> SurfaceSpec {
    engine
        .initial_commands()
        .expect("commands")
        .into_iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface),
            _ => None,
        })
        .expect("a surface")
}

fn row_interaction(nodes: &[PresentationNode], title: &str) -> Option<InteractionId> {
    nodes.iter().find_map(|node| match node {
        PresentationNode::List { rows, .. } => rows
            .iter()
            .find(|row| row.title == title)
            .and_then(|row| row.activation.as_ref())
            .map(|action| action.interaction_id.clone()),
        PresentationNode::Group { children, .. } => row_interaction(children, title),
        _ => None,
    })
}

// @internal
#[test]
fn tapping_cancel_on_hover_leaves_the_exchange() {
    let mut engine = engine_on_hover();
    let surface = presented(&mut engine);
    let cancel = row_interaction(&surface.nodes, "Cancel").expect("Hover offers Cancel");

    engine
        .dispatch(Event::ActionActivated {
            surface_id: surface.surface_id.clone(),
            interaction_id: cancel,
        })
        .expect("the tap is accepted");

    assert!(
        !matches!(
            engine.current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        ),
        "still on {:?}",
        engine.current_app_screen()
    );
}
