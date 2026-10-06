// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The app engine keeps the layout the peer last read for the next exchange
//! on this device, in memory, so a second exchange with the phones held the
//! same way starts its sweep where the first settled (#450, plan 2.9).
//!
//! Deterministic: each engine runs on a `FakeClock` advanced explicitly (CC-06).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::platform::{Command, PresentationNode, PresentationQrPurpose, QrPlacement};

/// Layout 6: 65 % of the square, top row, middle column.
fn the_only_layout_the_peer_reads() -> QrPlacement {
    QrPlacement::new(650, 175, 50).expect("placement inside the square")
}

fn own_qr_data(engine: &AppEngine) -> Option<String> {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|c| match c {
            Component::QrCode { id, data, .. } if id == "own_qr" => Some(data.clone()),
            _ => None,
        })
}

fn own_code_placement(engine: &mut AppEngine) -> Option<QrPlacement> {
    fn walk(nodes: &[PresentationNode]) -> Option<Option<QrPlacement>> {
        nodes.iter().find_map(|node| match node {
            PresentationNode::Qr {
                purpose: PresentationQrPurpose::Display,
                placement,
                ..
            } => Some(*placement),
            PresentationNode::Group { children, .. } => walk(children),
            _ => None,
        })
    }
    engine
        .initial_commands()
        .expect("commands")
        .into_iter()
        .rev()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => walk(&surface.nodes),
            _ => None,
        })
        .expect("the exchange shows its own code")
}

fn open_hover(engine: &mut AppEngine) {
    engine.navigate_to(AppScreen::Exchange);
    let _ = engine.handle_action(UserAction::ListItemSelected {
        component_id: "category:quick".into(),
        item_id: "mode:hover".into(),
    });
    assert!(matches!(
        engine.current_app_screen(),
        AppScreen::MultiStageExchange { .. }
    ));
}

fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut engine = AppEngine::new(vauchi);
    open_hover(&mut engine);
    engine
}

fn scan_into(engine: &mut AppEngine, qr: String) {
    let event = engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: qr });
    engine.apply_multi_stage_event(event);
}

// @internal
#[test]
fn a_second_exchange_starts_at_the_layout_the_first_one_settled_on() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let fake_a = Arc::new(FakeClock::new(start));
    let fake_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", fake_a.clone());
    let mut bob = engine_on_hover("Bob", fake_b.clone());
    let readable = the_only_layout_the_peer_reads();

    let mut saved = false;
    for _ in 0..2000 {
        alice.poll_notifications();
        bob.poll_notifications();
        let alice_placement = own_code_placement(&mut alice);
        if let Some(frame) = own_qr_data(&bob) {
            scan_into(&mut alice, frame);
        }
        if alice_placement == Some(readable)
            && let Some(frame) = own_qr_data(&alice)
        {
            scan_into(&mut bob, frame);
        }
        fake_a.advance(Duration::from_millis(100));
        fake_b.advance(Duration::from_millis(100));
        if !alice.vauchi().list_contacts().unwrap().is_empty() {
            saved = true;
            break;
        }
    }
    assert!(
        saved,
        "the exchange completes through the one readable layout"
    );

    open_hover(&mut alice);
    alice.poll_notifications();

    assert_eq!(own_code_placement(&mut alice), Some(readable));
}
