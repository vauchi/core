// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

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
    engine_on_hover_with_profile(vauchi)
}

fn engine_on_hover_with_profile(vauchi: Vauchi) -> AppEngine {
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
fn scan_into(engine: &mut AppEngine, qr: String) {
    let event = engine.forward_multi_stage_hardware_event(&Event::QrScanned { data: qr });
    engine.apply_multi_stage_event(event);
}

fn reveal_without_confirmation(qr: String) -> Option<String> {
    use vauchi_core::exchange::multistage::qr_codec::{StageQr, format_reveal_qr, parse_qr};
    match parse_qr(&qr).unwrap() {
        StageQr::Final {
            session_id,
            reveal_key,
            ..
        }
        | StageQr::Done {
            session_id,
            reveal_key,
            ..
        } => Some(format_reveal_qr(&session_id, &reveal_key)),
        StageQr::Both { .. } => None,
        _ => Some(qr),
    }
}

fn finalize_alice_before_bob(
    alice: &mut AppEngine,
    bob: &mut AppEngine,
    fake_a: &FakeClock,
    fake_b: &FakeClock,
    bob_id: &str,
) {
    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();
        let a_qr = own_qr_data(alice).and_then(reveal_without_confirmation);
        let b_qr = own_qr_data(bob);
        if let Some(frame) = b_qr {
            scan_into(alice, frame);
        }
        if let Some(frame) = a_qr {
            scan_into(bob, frame);
        }
        if alice
            .vauchi()
            .get_contact(bob_id)
            .unwrap()
            .unwrap()
            .display_name()
            == "Bob"
        {
            return;
        }
        fake_a.advance(Duration::from_millis(500));
        fake_b.advance(Duration::from_millis(500));
    }
    panic!("Alice did not finalize while Bob was waiting for her confirmation");
}

// @internal
#[test]
fn a_restored_contact_retries_ratchet_adoption_after_an_authenticated_peer_save() {
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let fake_a = Arc::new(FakeClock::new(start));
    let fake_b = Arc::new(FakeClock::new(start));
    let mut bob = engine_on_hover("Bob", fake_b.clone());
    let bob_pk = *bob.vauchi().identity().unwrap().signing_public_key();
    let old_key = vauchi_core::SymmetricKey::generate();
    let old_bytes = *old_key.as_bytes();
    let mut profile = Vauchi::in_memory_with_clock(fake_a.clone()).unwrap();
    profile.create_identity("Alice").unwrap();
    let alice_pk = *profile.identity().unwrap().signing_public_key();
    let restored = vauchi_core::Contact::from_exchange(
        bob_pk,
        vauchi_core::ContactCard::new("Bob before meeting"),
        old_key,
        0,
    );
    let bob_id = restored.id().to_owned();
    profile
        .storage()
        .contacts()
        .save_contact(&restored)
        .unwrap();
    profile
        .storage()
        .recovery()
        .mark_rekey_pending(&bob_id)
        .unwrap();
    let mut alice = engine_on_hover_with_profile(profile);
    finalize_alice_before_bob(&mut alice, &mut bob, &fake_a, &fake_b, &bob_id);
    assert_eq!(
        alice
            .vauchi()
            .get_contact(&bob_id)
            .unwrap()
            .unwrap()
            .shared_key()
            .unwrap()
            .as_bytes(),
        &old_bytes
    );
    assert!(
        !alice
            .vauchi()
            .storage()
            .recovery()
            .rekey_met_again(&bob_id)
            .unwrap()
    );
    assert!(bob.vauchi().list_contacts().unwrap().is_empty());
    alice
        .vauchi()
        .storage()
        .connection()
        .execute_batch(
            "CREATE TEMP TRIGGER reject_exchange_ratchet BEFORE INSERT ON contact_ratchets
         BEGIN SELECT RAISE(ABORT, 'forced exchange ratchet failure'); END;",
        )
        .unwrap();
    let mut authenticated_ending = false;

    for _ in 0..600 {
        alice.poll_notifications();
        bob.poll_notifications();
        let a_qr = own_qr_data(&alice);
        let b_qr = own_qr_data(&bob);
        if let Some(frame) = b_qr {
            authenticated_ending = frame.starts_with("DON4") || frame.starts_with("BTH4");
            scan_into(&mut alice, frame);
        }
        if let Some(frame) = a_qr {
            scan_into(&mut bob, frame);
        }
        alice.poll_notifications();
        if authenticated_ending {
            break;
        }
        fake_a.advance(Duration::from_millis(500));
        fake_b.advance(Duration::from_millis(500));
    }
    assert!(
        authenticated_ending,
        "Bob saved and sent an authenticated ending"
    );
    let still_old = alice.vauchi().get_contact(&bob_id).unwrap().unwrap();
    assert_eq!(still_old.shared_key().unwrap().as_bytes(), &old_bytes);
    assert!(
        !alice
            .vauchi()
            .storage()
            .recovery()
            .rekey_met_again(&bob_id)
            .unwrap()
    );

    alice
        .vauchi()
        .storage()
        .connection()
        .execute_batch("DROP TRIGGER reject_exchange_ratchet")
        .unwrap();
    alice.poll_notifications();

    let adopted = alice.vauchi().get_contact(&bob_id).unwrap().unwrap();
    let bob_view = bob
        .vauchi()
        .get_contact(&hex::encode(alice_pk))
        .unwrap()
        .unwrap();
    assert_ne!(adopted.shared_key().unwrap().as_bytes(), &old_bytes);
    assert_eq!(
        adopted.shared_key().unwrap().as_bytes(),
        bob_view.shared_key().unwrap().as_bytes()
    );
    assert!(
        alice
            .vauchi()
            .storage()
            .recovery()
            .rekey_met_again(&bob_id)
            .unwrap()
    );
    let (_, initiator) = alice
        .vauchi()
        .storage()
        .ratchets()
        .load_ratchet_state(&bob_id)
        .unwrap()
        .expect("retry persisted its ratchet");
    assert_eq!(
        initiator,
        vauchi_core::exchange::key_order::is_initiator(&alice_pk, &bob_pk)
    );
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
