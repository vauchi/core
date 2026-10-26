// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A shell's refresh gesture and a returning network reach Core as
//! observations, and Core decides what they mean, so no shell reads a sync
//! result to build its own feedback (vauchi/private#762).

use vauchi_app::ui::{AppEngine, Component, IndicatorKind, WorkflowEngine};
use vauchi_core::api::Vauchi;
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::{Command, Event, ImportSource};

fn engine_with_contact() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().unwrap();
    vauchi.create_identity("Alice").unwrap();
    let engine = AppEngine::new(vauchi);
    let contact = Contact::from_import(
        "contact-bob".to_string(),
        ContactCard::new("Bob"),
        ImportSource::VcardFile,
        None,
        0,
    );
    engine.vauchi().add_contact(contact).unwrap();
    engine
}

fn sync_chip_kind(engine: &AppEngine) -> Option<IndicatorKind> {
    engine
        .current_screen()
        .components
        .into_iter()
        .find_map(|component| match component {
            Component::Indicator { id, kind, .. } if id == "sync" => Some(kind),
            _ => None,
        })
}

fn wakeups(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, Command::ScheduleWakeup { .. }))
        .count()
}

// @internal
#[test]
fn a_refresh_gesture_runs_a_sync_and_presents_its_outcome() {
    let mut engine = engine_with_contact();
    assert_eq!(sync_chip_kind(&engine), Some(IndicatorKind::Neutral));

    let commands = engine
        .dispatch(Event::RefreshRequested)
        .expect("a refresh is a valid event");

    // No relay is reachable in tests, so the sync Core runs fails, and
    // the chip shows it.
    assert_eq!(sync_chip_kind(&engine), Some(IndicatorKind::Error));
    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::ReplaceSurface { .. })),
        "the outcome is presented: {commands:?}"
    );
}

// @internal
#[test]
fn coming_back_online_reschedules_the_heartbeat_at_once() {
    let mut engine = engine_with_contact();
    engine.set_network_online(false);
    let _ = engine.drain_pending_commands();

    engine.set_network_online(true);

    assert_eq!(wakeups(&engine.drain_pending_commands()), 1);
}

// @internal
#[test]
fn an_unchanged_network_does_not_reschedule() {
    let mut engine = engine_with_contact();
    engine.set_network_online(true);
    let _ = engine.drain_pending_commands();

    engine.set_network_online(true);
    engine.set_network_online(false);

    assert_eq!(wakeups(&engine.drain_pending_commands()), 0);
}
