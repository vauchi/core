// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! AppEngine-level tests for persist-at-Complete on the legacy QR
//! `ExchangeEngine` (`2026-06-04-exchange-terminal-screens`, robustness
//! follow-up to the reciprocity data-loss fix).
//!
//! The hook saves the received contact + ratchet the moment the QR session
//! reaches `Complete` — before the user taps Done — so a crash or a `Pending`
//! reciprocity outcome no longer strands a completed exchange. A persist-once
//! guard prevents a second save from resetting the Double Ratchet state.

use super::{AppEngine, AppScreen};
use crate::ui::exchange::{ExchangeConfig, ExchangeEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::SystemClock;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::exchange::{
    ExchangeEvent, ExchangeSession, ManualConfirmationVerifier, ProximityConfidence,
};
use vauchi_core::identity::Identity;

/// Drives a real QR `ExchangeSession` to `Complete` with confirmation-escrow
/// tokens available, mirroring the engine-level `qr_session_at_complete`
/// helper (kept local — the engine test mod is private to `exchange/`).
fn qr_session_at_complete() -> (ExchangeSession, String) {
    let clock = SystemClock::shared();
    let mut alice = ExchangeSession::new_qr(
        Identity::create("Alice", 0),
        ContactCard::new("Alice"),
        ManualConfirmationVerifier::new(),
        clock.clone(),
    );
    let mut bob = ExchangeSession::new_qr(
        Identity::create("Bob", 1),
        ContactCard::new("Bob"),
        ManualConfirmationVerifier::new(),
        clock.clone(),
    );
    alice.apply(ExchangeEvent::StartQR).unwrap();
    bob.apply(ExchangeEvent::StartQR).unwrap();
    let qr_bob = bob.qr().unwrap().clone();
    alice.apply(ExchangeEvent::ProcessQR(qr_bob)).unwrap();
    alice.apply(ExchangeEvent::TheyScannedOurQR).unwrap();
    alice
        .apply(ExchangeEvent::ProximityCheckCompleted {
            confidence: ProximityConfidence::Medium,
        })
        .unwrap();
    alice.apply(ExchangeEvent::PerformKeyAgreement).unwrap();
    alice
        .apply(ExchangeEvent::CompleteExchange(ContactCard::new("Bob")))
        .unwrap();
    assert!(
        matches!(
            alice.state(),
            vauchi_core::exchange::ExchangeState::Complete { .. }
        ),
        "session must reach Complete, got {:?}",
        alice.state()
    );
    let gate = alice
        .confirmation_escrow()
        .expect("escrow tokens available at Complete")
        .0
        .to_string();
    (alice, gate)
}

fn config_no_groups() -> ExchangeConfig {
    ExchangeConfig {
        own_name: "Alice".into(),
        own_qr_data: "qr-data".into(),
        available_groups: vec![],
        device_capabilities: Default::default(),
        transport_readiness: Default::default(),
        mode: Some(ExchangeMode::Glance),
        last_used_group_ids: None,
        last_used_mode: None,
        card_snapshot: None,
        available_group_data: Vec::new(),
        locale: crate::i18n::Locale::English,
    }
}

/// An `AppEngine` sitting on the Exchange screen whose active engine is a
/// legacy `ExchangeEngine` with a session already at `Complete`.
fn app_with_exchange_at_complete() -> (AppEngine, String) {
    let vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    let mut app = AppEngine::new(vauchi);
    let (session, gate) = qr_session_at_complete();
    let engine = ExchangeEngine::with_session(config_no_groups(), session, SystemClock::shared());
    app.engine = Box::new(engine);
    app.screen = AppScreen::Exchange;
    (app, gate)
}

// @internal
#[test]
fn persist_at_complete_saves_contact_before_done() {
    let (mut app, gate) = app_with_exchange_at_complete();
    let gate_hash = hex::decode(&gate).expect("escrow gate is hex");

    assert_eq!(
        app.vauchi().list_contacts().expect("list").len(),
        0,
        "no contact before any event",
    );

    // Session is already Complete; the first escrow event drives the engine's
    // step sync. The persist-at-Complete hook must save the contact NOW —
    // before Done (complete_exchange) is ever invoked.
    let _ = app.handle_hardware_event(Event::RelayEscrowReady { gate_hash });

    let contacts = app.vauchi().list_contacts().expect("list");
    assert_eq!(
        contacts.len(),
        1,
        "persist-at-Complete must save the received contact before Done",
    );

    // The persist-once guard records exactly which contact was saved.
    let contact_id = contacts[0].id().to_string();
    assert_eq!(
        app.legacy_exchange_persisted.as_deref(),
        Some(contact_id.as_str()),
        "the persist-once guard records the persisted contact",
    );
}

// Persist-once guard: re-saving the ratchet on Done would reset Double
// Ratchet state (2026-06-04-exchange-terminal-screens). Proven by a
// resurrection check — deleting the contact after persist-at-Complete and
// verifying Done does NOT re-create it. A removed guard would re-run
// `update_contact` from the still-`Complete` session and bring the contact
// (and a fresh ratchet) back.
// @internal
#[test]
fn done_after_persist_at_complete_does_not_resurrect_deleted_contact() {
    let (mut app, gate) = app_with_exchange_at_complete();
    let gate_hash = hex::decode(&gate).expect("escrow gate is hex");

    let _ = app.handle_hardware_event(Event::RelayEscrowReady { gate_hash });
    let contacts = app.vauchi().list_contacts().expect("list");
    assert_eq!(contacts.len(), 1, "persist-at-Complete saved the contact");
    let contact_id = contacts[0].id().to_string();

    // User deletes the just-created contact before tapping Done.
    assert!(
        app.vauchi().remove_contact(&contact_id).expect("remove"),
        "the persisted contact exists and is removed",
    );
    assert_eq!(app.vauchi().list_contacts().expect("list").len(), 0);

    // Done must be a no-op persist (the guard short-circuits): the deleted
    // contact stays deleted.
    let _ = app.complete_exchange();
    assert_eq!(
        app.vauchi().list_contacts().expect("list").len(),
        0,
        "Done must not resurrect a deleted contact — persist-once guard",
    );
}
