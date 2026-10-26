// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "testing")]

use std::sync::Arc;
use std::time::{Duration, SystemTime};
use vauchi_app::ui::{AppEngine, AppScreen, FormDialogType, UserAction, WorkflowEngine};
use vauchi_core::clock::FakeClock;
use vauchi_core::{Command, Event, NotificationSpec, NotificationUrgency, Vauchi, VauchiEvent};

fn engine() -> AppEngine {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock).unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn wakeup(engine: &mut AppEngine) -> Vec<Command> {
    let event = vauchi_core::event_from_json(r#""WakeupFired""#).unwrap();
    engine.dispatch(event).unwrap()
}

fn alert(engine: &AppEngine, nonce: u8) {
    engine
        .vauchi()
        .dispatch_event(VauchiEvent::EmergencyAlertReceived {
            contact_id: "bob".into(),
            message: "HELP".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [nonce; 32],
        });
}

fn notifications(commands: &[Command]) -> Vec<&NotificationSpec> {
    commands
        .iter()
        .filter_map(|command| match command {
            Command::PostNotification { notification } => Some(notification),
            _ => None,
        })
        .collect()
}

// @internal — canonical wakeups carry both prepared effects and the next timer (#764).
#[test]
fn wakeup_dispatch_posts_prepared_alert_and_drains_schedule_once() {
    let mut engine = engine();
    alert(&engine, 7);
    let commands = wakeup(&mut engine);
    assert_eq!(
        notifications(&commands),
        vec![&NotificationSpec {
            notification_id: Some(format!("alert:bob:{}", hex::encode([7; 32]))),
            title: "Emergency Alert".into(),
            body: "Contact bob sent an emergency alert".into(),
            deep_link_uri: Some("vauchi://contact/bob".into()),
            category_id: "emergency_alert".into(),
            channel_id: "alerts".into(),
            urgency: NotificationUrgency::Urgent,
            category_options: vec!["custom_dismiss_action".into()],
        }]
    );
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::ScheduleWakeup { .. }))
            .count(),
        1
    );
    assert_eq!(engine.drain_pending_commands(), Vec::new());
    assert_eq!(
        notifications(&wakeup(&mut engine)),
        Vec::<&NotificationSpec>::new()
    );
    assert_eq!(engine.poll_notifications(), Vec::new());
}

// @internal — old and new shells share the same persisted handoff ledger (#764).
#[test]
fn wakeup_dispatch_does_not_replay_a_legacy_handoff() {
    let mut engine = engine();
    alert(&engine, 1);
    assert_eq!(engine.drain_pending_notifications().len(), 1);
    assert_eq!(
        notifications(&wakeup(&mut engine)),
        Vec::<&NotificationSpec>::new()
    );
    alert(&engine, 2);
    let commands = wakeup(&mut engine);
    assert_eq!(notifications(&commands).len(), 1);
    assert_eq!(
        notifications(&commands)[0].notification_id,
        Some(format!("alert:bob:{}", hex::encode([2; 32])))
    );
    assert_eq!(engine.on_wakeup(), Vec::new());
}

// @internal — a new delivery envelope must retain configured notification policy (#764).
#[test]
fn wakeup_dispatch_respects_disabled_updates_without_replaying_them() {
    let mut engine = engine();
    engine.vauchi_mut().config_mut().card_update_notifications = false;
    engine.vauchi().dispatch_event(VauchiEvent::IncomingUpdate {
        contact_id: "bob".into(),
    });
    assert_eq!(
        notifications(&wakeup(&mut engine)),
        Vec::<&NotificationSpec>::new()
    );
    engine.vauchi_mut().config_mut().card_update_notifications = true;
    assert_eq!(
        notifications(&wakeup(&mut engine)),
        Vec::<&NotificationSpec>::new()
    );
}

// @internal — timer delivery cannot rebuild an unchanged form or discard its draft (#764).
#[test]
fn wakeup_dispatch_preserves_unsaved_form_input() {
    let mut engine = engine();
    engine.navigate_to(AppScreen::FormDialog {
        dialog_type: FormDialogType::EditName {
            current_name: "Alice".into(),
        },
    });
    engine.handle_action(UserAction::TextChanged {
        component_id: "display_name".into(),
        value: "Edited Alice".into(),
    });
    let commands = wakeup(&mut engine);
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command, Command::ReplaceSurface { .. }))
            .count(),
        0
    );
    engine.handle_action(UserAction::ActionPressed {
        action_id: "submit".into(),
    });
    assert_eq!(
        engine.vauchi().own_card().unwrap().unwrap().display_name(),
        "Edited Alice"
    );
}

// @internal — the next activation still closes the overlay opened before a timer (#764).
#[test]
fn wakeup_dispatch_preserves_open_overlay() {
    let mut engine = engine();
    let (surface_id, interaction_id) = engine
        .initial_commands()
        .unwrap()
        .into_iter()
        .find_map(|command| match command {
            Command::SetContextBar {
                surface_id, bar, ..
            } => bar
                .navigation
                .map(|action| (surface_id, action.interaction_id)),
            _ => None,
        })
        .unwrap();
    let activate = Event::ActionActivated {
        surface_id,
        interaction_id,
    };
    let opened = engine.dispatch(activate.clone()).unwrap();
    assert_eq!(
        opened
            .iter()
            .filter(|command| matches!(command, Command::PresentOverlay { .. }))
            .count(),
        1
    );
    let commands = wakeup(&mut engine);
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(
                command,
                Command::DismissOverlay { .. } | Command::ReplaceSurface { .. }
            ))
            .count(),
        0
    );
    let closed = engine.dispatch(activate).unwrap();
    assert_eq!(
        closed
            .iter()
            .filter(|command| matches!(command, Command::DismissOverlay { .. }))
            .count(),
        1
    );
    assert_eq!(
        closed
            .iter()
            .filter(|command| matches!(command, Command::PresentOverlay { .. }))
            .count(),
        0
    );
}
