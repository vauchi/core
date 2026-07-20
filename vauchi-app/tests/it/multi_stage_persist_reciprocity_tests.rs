// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic two-party multi-stage exchange at the AppEngine layer: drives
//! two engines through a mutual QR handshake to Finalized + persist, and asserts
//! the persisted contact is recorded as reciprocity `Pending`
//! (P3 — multi-stage is sync-resolvable; `2026-06-04-exchange-terminal-screens`).
//!
//! Fully deterministic — a `FakeClock` per engine is advanced explicitly past
//! each frame window, so there are no wall-clock waits (CC-06).

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::{AppEngine, AppScreen, Component, UserAction, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::clock::{Clock, FakeClock};
use vauchi_core::exchange::reciprocity::Reciprocity;

/// The `own_qr` display payload on the engine's current screen, if any.
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

/// An AppEngine on the multi-stage (Hover) exchange screen, on a shared clock.
fn engine_on_hover(name: &str, clock: Arc<dyn Clock>) -> AppEngine {
    let mut vauchi = Vauchi::in_memory_with_clock(clock).expect("in-memory Vauchi");
    vauchi.create_identity(name).expect("identity");
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
        "{name}: Hover must land on MultiStageExchange, got {:?}",
        engine.current_app_screen()
    );
    engine
}

/// Feed `qr` (a peer's displayed frame) into `engine` as a scan.
fn scan_into(engine: &mut AppEngine, qr: String) {
    let event = engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: qr });
    engine.apply_multi_stage_event(event);
}

// P3: a multi-stage exchange persists both contacts as reciprocity Pending —
// confirmable (shared key derives tokens) and sync-resolvable, not silently
// mutual. Closes the tested-coverage gap on the persist-time stamp.
// @scenario: multi_stage_exchange :: persisted contacts are recorded Pending
// @internal
#[test]
fn two_party_multi_stage_persists_contacts_as_pending() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let fake_a = Arc::new(FakeClock::new(start));
    let fake_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", fake_a.clone());
    let mut bob = engine_on_hover("Bob", fake_b.clone());

    // Drive: poll each (emit its current frame), cross-feed frames, advance both
    // clocks one frame-window, until both persist a contact (Finalized) or bound.
    let mut finalized = false;
    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();
        let a_qr = own_qr_data(&alice);
        let b_qr = own_qr_data(&bob);
        if let Some(d) = b_qr {
            scan_into(&mut alice, d);
        }
        if let Some(d) = a_qr {
            scan_into(&mut bob, d);
        }
        fake_a.advance(Duration::from_millis(500));
        fake_b.advance(Duration::from_millis(500));

        if !alice.vauchi().list_contacts().unwrap().is_empty()
            && !bob.vauchi().list_contacts().unwrap().is_empty()
        {
            finalized = true;
            break;
        }
    }
    assert!(
        finalized,
        "the two-party multi-stage exchange must reach Finalized + persist within the bound"
    );

    let now = alice.vauchi().clock().unix_seconds();
    let alice_contacts = alice.vauchi().list_contacts().expect("list");
    assert_eq!(
        alice_contacts.len(),
        1,
        "Alice persisted exactly one contact"
    );
    assert_eq!(
        alice_contacts[0].reciprocity(now),
        Reciprocity::Pending,
        "a multi-stage exchange records the contact as Pending (confirmable, sync-resolvable)"
    );

    let bob_contacts = bob.vauchi().list_contacts().expect("list");
    assert_eq!(bob_contacts.len(), 1, "Bob persisted exactly one contact");
    assert_eq!(
        bob_contacts[0].reciprocity(bob.vauchi().clock().unix_seconds()),
        Reciprocity::Pending,
        "the other side records Pending too"
    );
}

// Shape-(b1) characterization (consolidation Step 1): multi-stage persists
// via bare `Contact::from_exchange` + `save_exchanged_contact`. Two pins:
// the transport is stamped `Qr` (no `MultiStage` variant exists — a
// multi-stage contact is structurally indistinguishable from a QR one
// today; Step 2 changes this deliberately, updating THIS test), and the
// ratchet row exists with the canonical smaller-identity role flag.
// @scenario: multi_stage_exchange :: persisted contact carries Qr transport and a role-correct ratchet
// @internal
#[test]
fn two_party_multi_stage_persists_qr_transport_and_role_correct_ratchet() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let fake_a = Arc::new(FakeClock::new(start));
    let fake_b = Arc::new(FakeClock::new(start));
    let mut alice = engine_on_hover("Alice", fake_a.clone());
    let mut bob = engine_on_hover("Bob", fake_b.clone());

    let mut finalized = false;
    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();
        let a_qr = own_qr_data(&alice);
        let b_qr = own_qr_data(&bob);
        if let Some(d) = b_qr {
            scan_into(&mut alice, d);
        }
        if let Some(d) = a_qr {
            scan_into(&mut bob, d);
        }
        fake_a.advance(Duration::from_millis(500));
        fake_b.advance(Duration::from_millis(500));

        if !alice.vauchi().list_contacts().unwrap().is_empty()
            && !bob.vauchi().list_contacts().unwrap().is_empty()
        {
            finalized = true;
            break;
        }
    }
    assert!(finalized, "two-party multi-stage exchange must persist");

    let alice_id = *alice
        .vauchi()
        .identity()
        .expect("alice identity")
        .signing_public_key();
    let bob_id = *bob
        .vauchi()
        .identity()
        .expect("bob identity")
        .signing_public_key();

    let alices_bob = &alice.vauchi().list_contacts().expect("list")[0];
    assert_eq!(
        alices_bob.exchange_transport(),
        Some(vauchi_core::types::ExchangeTransport::Qr),
        "pin: multi-stage stamps Qr today (no MultiStage variant) — \
         Step 2 changes this deliberately"
    );

    let (_ratchet, a_init) = alice
        .vauchi()
        .storage()
        .ratchets()
        .load_ratchet_state(alices_bob.id())
        .expect("load ok")
        .expect("multi-stage persisted a ratchet row");
    assert_eq!(
        a_init,
        vauchi_core::exchange::key_order::is_initiator(&alice_id, &bob_id),
        "persisted role flag matches the canonical role rule"
    );
}
