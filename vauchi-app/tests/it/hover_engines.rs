// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Two AppEngines on the Hover screen and the frames that pass between
//! them, for the two-party tests at the engine layer.

use std::sync::Arc;

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::Clock;

/// The `own_qr` display payload on the engine's current screen, if any.
pub fn own_qr_data(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::QrCode { id, data, .. } if id == "own_qr" => Some(data.clone()),
            _ => None,
        })
}

/// An AppEngine on the multi-stage (Hover) exchange screen, on a shared clock.
pub fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    engine_on_hover_with_profile(vauchi)
}

pub fn engine_on_hover_with_profile(vauchi: Vauchi) -> AppEngine {
    let mut engine = AppEngine::new(vauchi);
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert!(
        matches!(
            engine.current_app_screen(),
            AppScreen::MultiStageExchange { .. }
        ),
        "Hover must land on MultiStageExchange, got {:?}",
        engine.current_app_screen()
    );
    engine
}

/// Feed `qr` (a peer's displayed frame) into `engine` as a scan.
pub fn scan_into(engine: &mut AppEngine, qr: String) {
    let event = engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: qr });
    engine.apply_multi_stage_event(event);
}
