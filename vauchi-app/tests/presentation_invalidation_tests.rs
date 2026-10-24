// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::mpsc::TryRecvError;
use vauchi_app::ui::AppEngine;
use vauchi_core::{Command, Event, NotificationUrgency, Vauchi, VauchiEvent};

fn app() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

// @internal — a shell must not need a second domain API to obtain notifications.
#[test]
fn invalidation_returns_pending_notification_once_with_the_refreshed_surface() {
    let mut app = app();
    app.initial_commands().unwrap();
    app.vauchi()
        .dispatch_event(VauchiEvent::EmergencyAlertReceived {
            contact_id: "bob".into(),
            message: "HELP".into(),
            timestamp: 1_700_000_000,
            location: None,
            alert_nonce: [7; 32],
        });

    let commands = app.dispatch(Event::PresentationInvalidated).unwrap();
    assert!(commands.iter().any(|command| matches!(command,
        Command::ReplaceSurface { surface } if surface.surface_id.as_str() == "my_info"
    )));
    let notifications: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            Command::PostNotification { notification } => Some(notification),
            _ => None,
        })
        .collect();
    assert_eq!(notifications.len(), 1);
    assert_eq!(notifications[0].title, "Emergency Alert");
    assert_eq!(notifications[0].body, "Contact bob sent an emergency alert");
    assert_eq!(notifications[0].urgency, NotificationUrgency::Urgent);

    let repeated = app.dispatch(Event::PresentationInvalidated).unwrap();
    assert_eq!(
        repeated
            .iter()
            .filter(|command| matches!(command, Command::PostNotification { .. }))
            .count(),
        0
    );
    assert_eq!(app.drain_pending_notifications(), Vec::new());
}

// @internal — polling shells receive one opaque wakeup, never domain payloads.
#[test]
fn presentation_subscription_coalesces_events_without_blocking_the_engine() {
    let app = app();
    let receiver = app.subscribe_presentation_invalidations();
    assert_eq!(
        receiver.try_recv(),
        Ok(()),
        "registration requests a refresh"
    );
    for _ in 0..100 {
        app.vauchi().dispatch_event(VauchiEvent::ContentUpdated);
    }
    assert_eq!(receiver.try_recv(), Ok(()));
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Empty));
}

// @internal — replacing a shell drops the old sender and preserves a pending refresh.
#[test]
fn replacement_disconnects_the_old_subscription() {
    let app = app();
    let old = app.subscribe_presentation_invalidations();
    assert_eq!(old.try_recv(), Ok(()));
    let current = app.subscribe_presentation_invalidations();
    app.vauchi().dispatch_event(VauchiEvent::ContentUpdated);
    assert_eq!(old.try_recv(), Err(TryRecvError::Disconnected));
    assert_eq!(current.try_recv(), Ok(()));
    assert_eq!(current.try_recv(), Err(TryRecvError::Empty));
}

// @internal — native receiver disposal must not break Core's event handling.
#[test]
fn discarded_subscription_can_be_replaced_after_an_event() {
    let app = app();
    drop(app.subscribe_presentation_invalidations());
    app.vauchi().dispatch_event(VauchiEvent::ContentUpdated);
    let current = app.subscribe_presentation_invalidations();
    assert_eq!(current.try_recv(), Ok(()));
    assert_eq!(current.try_recv(), Err(TryRecvError::Empty));
}

// @internal — a surviving shell receiver cannot retain its engine's sender.
#[test]
fn engine_disposal_disconnects_the_subscription() {
    let app = app();
    let receiver = app.subscribe_presentation_invalidations();
    assert_eq!(receiver.try_recv(), Ok(()));
    drop(app);
    assert_eq!(receiver.try_recv(), Err(TryRecvError::Disconnected));
}
