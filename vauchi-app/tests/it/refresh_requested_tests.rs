// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A shell's refresh gesture and a returning network reach Core as
//! observations, and Core decides what they mean, so no shell reads a sync
//! result to build its own feedback (vauchi/private#762).

use vauchi_app::ui::{AppEngine, Component, IndicatorKind, WorkflowEngine};
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
use vauchi_core::{Command, Event, ImportSource, SurfaceId};

/// The relays are unreachable, so no test touches the network: a sync
/// Core runs can only fail.
fn engine_with_contact() -> (AppEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url("http://127.0.0.1:1")
        .with_ohttp_relay_url("http://127.0.0.1:2");
    let mut vauchi = Vauchi::new(config).expect("vauchi");
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
    (engine, dir)
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

fn presented_surface(engine: &mut AppEngine) -> SurfaceId {
    engine
        .initial_commands()
        .expect("initial commands")
        .into_iter()
        .find_map(|command| match command {
            Command::ReplaceSurface { surface } => Some(surface.surface_id),
            _ => None,
        })
        .expect("a surface")
}

fn refresh(engine: &mut AppEngine) -> Vec<Command> {
    let surface_id = presented_surface(engine);
    engine
        .dispatch(Event::RefreshRequested { surface_id })
        .expect("a refresh of the presented surface is valid")
}

fn wakeups(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, Command::ScheduleWakeup { .. }))
        .count()
}

// @internal
#[test]
fn a_refresh_gesture_presents_the_current_surface_again() {
    let (mut engine, _dir) = engine_with_contact();

    let commands = refresh(&mut engine);

    assert!(
        commands
            .iter()
            .any(|command| matches!(command, Command::ReplaceSurface { .. })),
        "the outcome is presented: {commands:?}"
    );
}

// Sync exists only with the HTTP transport, as for the sync chip's tests.
#[cfg(feature = "network-http")]
// @internal
#[test]
fn a_refresh_gesture_runs_a_sync() {
    let (mut engine, _dir) = engine_with_contact();
    assert_eq!(sync_chip_kind(&engine), Some(IndicatorKind::Neutral));

    let _ = refresh(&mut engine);

    // The relays are unreachable, so the sync Core runs fails, and the
    // chip shows it.
    assert_eq!(sync_chip_kind(&engine), Some(IndicatorKind::Error));
}

// @internal
#[test]
fn a_refresh_of_a_surface_core_never_presented_is_rejected() {
    let (mut engine, _dir) = engine_with_contact();
    let _ = presented_surface(&mut engine);

    let result = engine.dispatch(Event::RefreshRequested {
        surface_id: SurfaceId::new("never-presented").unwrap(),
    });

    assert!(
        result.is_err(),
        "a stale or forged surface must fail closed: {result:?}"
    );
    assert_eq!(sync_chip_kind(&engine), Some(IndicatorKind::Neutral));
}

// @internal
#[test]
fn coming_back_online_reschedules_the_heartbeat_at_once() {
    let (mut engine, _dir) = engine_with_contact();
    engine.set_network_online(false);
    let _ = engine.drain_pending_commands();

    engine.set_network_online(true);

    assert_eq!(wakeups(&engine.drain_pending_commands()), 1);
}

// @internal
#[test]
fn an_unchanged_network_does_not_reschedule() {
    let (mut engine, _dir) = engine_with_contact();
    engine.set_network_online(true);
    let _ = engine.drain_pending_commands();

    engine.set_network_online(true);
    engine.set_network_online(false);

    assert_eq!(wakeups(&engine.drain_pending_commands()), 0);
}
