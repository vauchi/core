// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Back on the Hover screen ends the exchange the way Cancel does, so the
//! shells can drop Cancel where Back is shown in the title row (#534).

use vauchi_app::ui::{AppEngine, AppScreen, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::platform::Command;

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

// @internal
#[test]
fn back_on_hover_leaves_the_screen_and_ends_the_session() {
    let mut engine = engine_on_hover();
    assert!(
        engine.multi_stage_session_active(),
        "precondition: a live session"
    );
    let commands = engine.initial_commands().expect("commands");
    let (surface_id, back) = commands
        .iter()
        .rev()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .back
                .as_ref()
                .map(|back| (surface_id.clone(), back.interaction_id.clone())),
            _ => None,
        })
        .expect("Hover offers Back");

    engine
        .dispatch(Event::ActionActivated {
            surface_id,
            interaction_id: back,
        })
        .expect("the tap is accepted");

    assert!(!matches!(
        engine.current_app_screen(),
        AppScreen::MultiStageExchange { .. }
    ));
    assert!(!engine.multi_stage_session_active());
}
