// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A platform wakeup syncs with the relay when Core's sync gate says one is
//! due (vauchi/private#648, owner decision 2026-10-12). Before, `on_wakeup`
//! only advanced exchange sessions and polled the activity log, so on
//! Android nothing fetched mail unless the user tapped Sync.

#![cfg(feature = "network-http")]

use vauchi_app::ui::{AppEngine, Component, WorkflowEngine};
use vauchi_core::ImportSource;
use vauchi_core::api::{Vauchi, VauchiConfig};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;

/// Alice with one contact, pointed at relays that refuse connections, so a
/// sync attempt is visible as a failed sync chip.
fn engine_with_unreachable_relay() -> (AppEngine, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("tempdir");
    let config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(SymmetricKey::generate())
        .with_relay_url("http://127.0.0.1:1")
        .with_ohttp_relay_url("http://127.0.0.1:2");
    let mut vauchi = Vauchi::new(config).expect("vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let engine = AppEngine::new(vauchi);
    engine
        .vauchi()
        .add_contact(Contact::from_import(
            "contact-Bob".into(),
            ContactCard::new("Bob"),
            ImportSource::VcardFile,
            None,
            0,
        ))
        .unwrap();
    (engine, dir)
}

fn sync_chip_label(engine: &AppEngine) -> String {
    engine
        .current_screen()
        .components
        .iter()
        .find_map(|component| match component {
            Component::Indicator { id, label, .. } if id == "sync" => Some(label.clone()),
            _ => None,
        })
        .expect("sync indicator present")
}

// @internal
#[test]
fn a_wakeup_runs_a_due_sync() {
    let (mut engine, _dir) = engine_with_unreachable_relay();
    let idle = sync_chip_label(&engine);

    engine.on_wakeup();

    assert_eq!(idle, "Sync");
    assert_eq!(sync_chip_label(&engine), "Sync failed");
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn a_wakeup_inside_the_sync_gate_does_not_sync() {
    use std::time::{Duration, Instant};
    let (mut engine, _dir) = engine_with_unreachable_relay();
    engine
        .vauchi_mut()
        .set_next_sync_allowed(Instant::now() + Duration::from_secs(3600));

    engine.on_wakeup();

    assert_eq!(sync_chip_label(&engine), "Sync");
}
