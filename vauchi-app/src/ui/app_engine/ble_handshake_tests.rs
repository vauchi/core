// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Inline tests for `ble_handshake.rs` — extracted to keep the engine
//! file under the src size limit. Loaded via `#[path]`; stays a unit-test
//! child module (private-item access preserved).

// INLINE_TEST_REQUIRED: tests call the private `build_ble_session_inputs`
// and set the private `pending_exchange_groups` field — neither is reachable
// from a `tests/` integration directory.
use super::*;
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::platform::BleLinkDirection;

/// AppEngine over an in-memory Vauchi whose own card carries `Email` +
/// `Phone`, plus a "Work" group exposing only `Email`. Returns the engine
/// and the Work group id.
/// Resolves an own-card field label to its generated id.
fn own_field_id(vauchi: &Vauchi, label: &str) -> String {
    let card = vauchi.own_card().expect("own_card").expect("card present");
    let field = card.fields().iter().find(|f| f.label() == label);
    field.expect("labeled field").id().to_string()
}

fn engine_with_card_and_group() -> (AppEngine, String) {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    let mut card = vauchi
        .own_card()
        .expect("own_card")
        .expect("create_identity saves a card");
    card.add_field(ContactField::new(FieldType::Email, "Email", "a@b.com", 0))
        .expect("add email");
    card.add_field(ContactField::new(
        FieldType::Phone,
        "Phone",
        "+12025550123",
        0,
    ))
    .expect("add phone");
    vauchi.update_own_card(&card).expect("update own card");
    let email_id = own_field_id(&vauchi, "Email");
    let work = vauchi.create_group("Work").expect("create group");
    let work_id = work.id().to_string();
    vauchi
        .set_group_field_visibility(&work_id, &email_id, true)
        .expect("expose email to Work");
    (AppEngine::new(vauchi), work_id)
}

fn payload_labels(engine: &AppEngine) -> Vec<String> {
    let (_id, _x3dh, card) = engine
        .build_ble_session_inputs()
        .expect("identity + card present");
    card.fields.iter().map(|(label, _)| label.clone()).collect()
}

// @internal
#[test]
fn ble_payload_shares_visible_toggled_base_when_no_group_selected() {
    // No selection → curated base: the Visible-toggled unassigned Phone
    // ships; Work-assigned Email stays out (field-centric, 2026-07-10).
    let (engine, _work) = engine_with_card_and_group();
    engine
        .vauchi
        .set_own_field_public(&own_field_id(&engine.vauchi, "Phone"))
        .expect("toggle Phone visible");
    assert_eq!(payload_labels(&engine), vec!["Phone".to_string()]);
}

// @internal
#[test]
fn ble_payload_filtered_to_selected_group_visible_fields() {
    // Work exposes only Email; selecting it must drop Phone from the
    // transmitted BLE payload (the privacy fix).
    let (mut engine, work) = engine_with_card_and_group();
    engine.pending_exchange_groups = vec![work];
    let labels = payload_labels(&engine);
    assert_eq!(
        labels,
        vec!["Email".to_string()],
        "Work group exposes only Email; Phone must not be transmitted"
    );
}

// @internal
#[test]
fn ble_payload_empty_when_selected_group_exposes_nothing() {
    // Default-closed: a selected group with no visible_fields shares no
    // fields (Some(∅)), NOT the full card.
    let (mut engine, _work) = engine_with_card_and_group();
    let empty = engine
        .vauchi
        .create_group("Empty")
        .expect("create empty group");
    engine.pending_exchange_groups = vec![empty.id().to_string()];
    let labels = payload_labels(&engine);
    assert!(
        labels.is_empty(),
        "empty group → share nothing, got {labels:?}"
    );
}

/// Route one side's pending BLE writes to the other as notifications on
/// the same characteristic (a GATT write on uuid X surfaces at the peer
/// as data on uuid X), applying any resulting machine event (which
/// persists the contact on `Completed`). Returns the writes routed.
fn pump(from: &mut AppEngine, to: &mut AppEngine) -> usize {
    let mut routed = 0;
    for cmd in from.drain_pending_commands() {
        if let vauchi_core::Command::BleWriteCharacteristic {
            device_id: _,
            direction,
            uuid,
            data,
        } = cmd
        {
            routed += 1;
            // Forward un-addressed (wildcard): the writer stamps ITS
            // link id, which names the receiver from the writer's side;
            // a real shell re-stamps with the receiver-side link id.
            // Link scoping has dedicated machine-level tests.
            let ev =
                to.forward_ble_hardware_event(&vauchi_core::Event::BleCharacteristicNotified {
                    device_id: String::new(),
                    direction,
                    uuid,
                    data,
                });
            to.apply_ble_machine_event(ev);
        }
    }
    routed
}

// @internal
#[test]
fn two_device_ble_exchange_peer_receives_only_group_visible_fields() {
    // End-to-end G4 ratchet: Alice shares to a Work group exposing only
    // Email; after a full two-device BLE exchange Bob's stored contact
    // card must carry Email and NOT Phone — the privacy guarantee.
    let (mut alice, work) = engine_with_card_and_group();
    alice.pending_exchange_groups = vec![work];

    let mut vauchi_bob = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi_bob.create_identity("Bob").expect("identity");
    let mut bob = AppEngine::new(vauchi_bob);

    let alice_token = alice
        .vauchi
        .identity()
        .expect("alice identity")
        .signing_public_key()
        .to_vec();
    let bob_token = bob
        .vauchi
        .identity()
        .expect("bob identity")
        .signing_public_key()
        .to_vec();

    // Each discovers the other → builds a session with the tiebreak role.
    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);

    // Connect both; the initiator emits its KeyOffer on connect.
    let ea = alice.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    bob.apply_ble_machine_event(eb);

    // Pump writes back and forth until the exchange settles.
    for _ in 0..50 {
        let a = pump(&mut alice, &mut bob);
        let b = pump(&mut bob, &mut alice);
        if a + b == 0 {
            break;
        }
    }

    let bob_contacts = bob.vauchi.list_contacts().expect("list contacts");
    assert_eq!(bob_contacts.len(), 1, "Bob should have exactly Alice");
    let alice_card = bob_contacts[0].card();
    let labels: Vec<&str> = alice_card.fields().iter().map(|f| f.label()).collect();
    assert!(
        labels.contains(&"Email"),
        "Email is in the Work group → must reach Bob; got {labels:?}"
    );
    assert!(
        !labels.contains(&"Phone"),
        "Phone is NOT in the Work group → must NOT reach Bob; got {labels:?}"
    );
}

// @scenario: ble_exchange :: Both peers persist the exchanged contact
#[test]
fn two_device_ble_exchange_persists_contact_for_both_roles() {
    // Regression guard for the iOS responder-persist bug
    // (2026-06-08-ios-ble-responder-persist): the responder reached
    // "Completed" but created no contact. Persistence is core-driven and
    // role-symmetric — BOTH the handshake initiator and responder must
    // create the peer contact. The role is decided by the identity
    // tiebreak, so asserting only one side (as the privacy test above
    // does) covers the responder path only ~half the time. Assert both:
    // whichever engine is the responder, its persist must succeed (a live
    // session key at completion).
    let mut va = Vauchi::in_memory().expect("vauchi alice");
    va.create_identity("Alice").expect("alice identity");
    let mut alice = AppEngine::new(va);

    let mut vb = Vauchi::in_memory().expect("vauchi bob");
    vb.create_identity("Bob").expect("bob identity");
    let mut bob = AppEngine::new(vb);

    let alice_token = alice
        .vauchi
        .identity()
        .expect("alice identity")
        .signing_public_key()
        .to_vec();
    let bob_token = bob
        .vauchi
        .identity()
        .expect("bob identity")
        .signing_public_key()
        .to_vec();

    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);

    let ea = alice.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    bob.apply_ble_machine_event(eb);

    for _ in 0..50 {
        let a = pump(&mut alice, &mut bob);
        let b = pump(&mut bob, &mut alice);
        if a + b == 0 {
            break;
        }
    }

    assert_eq!(
        alice.vauchi.list_contacts().expect("alice contacts").len(),
        1,
        "Alice must persist Bob after completion — 0 means the role she \
         played (initiator or responder) failed to persist"
    );
    assert_eq!(
        bob.vauchi.list_contacts().expect("bob contacts").len(),
        1,
        "Bob must persist Alice after completion — 0 means the role he \
         played (initiator or responder) failed to persist"
    );
}

// @scenario: ble_exchange :: Persisted ratchets from both roles form a working channel
#[test]
fn two_device_ble_exchange_persisted_ratchets_interoperate() {
    // Consolidation Step-1 pin (U3): the BLE persist path hand-rolls
    // its ratchet init instead of calling
    // `ratchet_bootstrap::bootstrap_exchange_ratchet`. This drives the
    // REAL two-device flow and proves the two persisted states form a
    // working bidirectional channel with complementary roles — the
    // contract the planned helper substitution must preserve. Recipe
    // equivalence itself is pinned in core
    // `tests/it/consolidation_pinning_tests.rs`.
    let mut va = Vauchi::in_memory().expect("vauchi alice");
    va.create_identity("Alice").expect("alice identity");
    let mut alice = AppEngine::new(va);

    let mut vb = Vauchi::in_memory().expect("vauchi bob");
    vb.create_identity("Bob").expect("bob identity");
    let mut bob = AppEngine::new(vb);

    let alice_token = alice
        .vauchi
        .identity()
        .expect("alice identity")
        .signing_public_key()
        .to_vec();
    let bob_token = bob
        .vauchi
        .identity()
        .expect("bob identity")
        .signing_public_key()
        .to_vec();

    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);

    let ea = alice.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    bob.apply_ble_machine_event(eb);

    for _ in 0..50 {
        let a = pump(&mut alice, &mut bob);
        let b = pump(&mut bob, &mut alice);
        if a + b == 0 {
            break;
        }
    }

    let alices_bob = alice.vauchi.list_contacts().expect("alice contacts");
    let bobs_alice = bob.vauchi.list_contacts().expect("bob contacts");
    assert_eq!(alices_bob.len(), 1, "alice persisted bob");
    assert_eq!(bobs_alice.len(), 1, "bob persisted alice");

    let (ra, a_init) = alice
        .vauchi
        .storage()
        .ratchets()
        .load_ratchet_state(alices_bob[0].id())
        .expect("alice load ok")
        .expect("alice persisted a ratchet");
    let (rb, b_init) = bob
        .vauchi
        .storage()
        .ratchets()
        .load_ratchet_state(bobs_alice[0].id())
        .expect("bob load ok")
        .expect("bob persisted a ratchet");
    assert_ne!(a_init, b_init, "exactly one side is the ratchet initiator");
    // Shape-(a) characterization (consolidation Step 1): BLE persists
    // via `from_exchange_full` + `save_exchanged_contact` — transport
    // stamped `Ble`, role flag equal to the canonical smaller-identity
    // rule the shared helper encodes.
    assert_eq!(
        alices_bob[0].exchange_transport(),
        Some(vauchi_core::types::ExchangeTransport::Ble),
        "BLE persist stamps its transport"
    );
    assert_eq!(
        a_init,
        alice_token < bob_token,
        "persisted role flag matches the canonical smaller-identity rule"
    );

    let (mut init_side, mut resp_side) = if a_init { (ra, rb) } else { (rb, ra) };
    let m1 = init_side.encrypt(b"probe-1").expect("initiator encrypts");
    assert_eq!(
        resp_side.decrypt(&m1).expect("responder decrypts"),
        b"probe-1".to_vec()
    );
    let m2 = resp_side.encrypt(b"probe-2").expect("responder replies");
    assert_eq!(
        init_side.decrypt(&m2).expect("initiator decrypts"),
        b"probe-2".to_vec()
    );
    let m3 = init_side
        .encrypt(b"probe-3")
        .expect("initiator crosses ratchet step");
    assert_eq!(
        resp_side.decrypt(&m3).expect("responder decrypts m3"),
        b"probe-3".to_vec()
    );
}

// @scenario: ble_exchange :: A peripheral responder (no discovery) persists the contact
#[test]
fn responder_built_on_connect_without_discovery_persists() {
    // Reproduces the iOS peripheral-responder path
    // (2026-06-08-ios-ble-responder-persist): the peripheral never emits
    // `BleDeviceDiscovered`, so it builds its session via
    // `start_ble_handshake_as_responder` (driven from `BleConnected` in
    // the platform layer) instead of `start_ble_handshake_on_discovery`.
    // It must still decrypt the peer card and persist the contact.
    let (mut initiator, _work) = engine_with_card_and_group();
    // Force the initiator role deterministically (the central would have
    // built this on discovery via the tiebreak).
    let (ik, x3dh, card) = initiator
        .build_ble_session_inputs()
        .expect("initiator inputs");
    initiator.ensure_ble_handshake_session(BleRole::Initiator, ik, x3dh, card, None);

    let mut vb = Vauchi::in_memory().expect("vauchi bob");
    vb.create_identity("Bob").expect("bob identity");
    let mut responder = AppEngine::new(vb);
    // The peripheral path: no discovery happened, so build as responder.
    responder.start_ble_handshake_as_responder();
    assert!(
        responder.ble_handshake_session_active(),
        "responder session must build without a prior discovery"
    );

    let ei = initiator.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    initiator.apply_ble_machine_event(ei);
    let er = responder.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    responder.apply_ble_machine_event(er);

    for _ in 0..50 {
        let a = pump(&mut initiator, &mut responder);
        let b = pump(&mut responder, &mut initiator);
        if a + b == 0 {
            break;
        }
    }

    assert_eq!(
        responder.vauchi.list_contacts().expect("contacts").len(),
        1,
        "a peripheral responder built on connect (no discovery) must \
         persist the peer contact"
    );
}

// ============================================================
// Glance Slice B — OOB binding supply (pin + nonce echo) reaches the
// session enforcement. The binding is the exposure-closer for
// 2026-06-10-ble-unauthenticated-peer-identity.
// ============================================================

fn fresh_engine(name: &str) -> AppEngine {
    let mut v = Vauchi::in_memory().expect("in-memory vauchi");
    v.create_identity(name).expect("identity");
    AppEngine::new(v)
}

fn signing_key(e: &AppEngine) -> [u8; 32] {
    *e.vauchi.identity().expect("identity").signing_public_key()
}

fn ensure_with_oob(e: &mut AppEngine, role: BleRole, oob: Option<BleOobBinding>) {
    let (ik, x3dh, card) = e.build_ble_session_inputs().expect("session inputs");
    e.ensure_ble_handshake_session(role, ik, x3dh, card, oob);
}

fn run_handshake(initiator: &mut AppEngine, responder: &mut AppEngine) {
    let ei = initiator.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "responder".into(),
        direction: BleLinkDirection::Outbound,
    });
    initiator.apply_ble_machine_event(ei);
    let er = responder.forward_ble_hardware_event(&vauchi_core::Event::BleConnected {
        device_id: "initiator".into(),
        direction: BleLinkDirection::Inbound,
    });
    responder.apply_ble_machine_event(er);
    for _ in 0..50 {
        let a = pump(initiator, responder);
        let b = pump(responder, initiator);
        if a + b == 0 {
            break;
        }
    }
}

// @scenario: ble_exchange :: Glance scanner rejects a foreign displayer
#[test]
fn glance_scanner_rejects_foreign_displayer_via_identity_pin() {
    // The scanner scanned the REAL displayer's QR and pinned its identity.
    // A radio-range MITM (Mallory) answers instead — she even knows the
    // co-presence nonce (worst case: the QR was shoulder-surfed) but not
    // the pinned identity's signing key, so the scanner must reject her.
    let mut scanner = fresh_engine("Scanner");
    let displayer = fresh_engine("RealDisplayer");
    let mut mallory = fresh_engine("Mallory");

    let pinned = signing_key(&displayer);
    let nonce = [5u8; 16];

    ensure_with_oob(
        &mut scanner,
        BleRole::Initiator,
        Some(BleOobBinding {
            expected_peer: Some(pinned),
            oob_nonce_echo: Some(nonce),
            ..Default::default()
        }),
    );
    ensure_with_oob(
        &mut mallory,
        BleRole::Responder,
        Some(BleOobBinding {
            required_oob_nonce: Some(nonce),
            ..Default::default()
        }),
    );

    run_handshake(&mut scanner, &mut mallory);

    assert_eq!(
        scanner.vauchi.list_contacts().expect("contacts").len(),
        0,
        "scanner pinned the scanned identity — Mallory's mismatched \
         identity must abort the handshake (no contact persisted)"
    );
}

// @scenario: ble_exchange :: Glance displayer rejects a connector that never scanned
#[test]
fn glance_displayer_rejects_connector_without_nonce_echo() {
    // The displayer requires the co-presence nonce it showed in its QR. A
    // connector that never scanned it (no echo) must be rejected — this is
    // what stops a non-co-present device from harvesting the displayer's
    // card by merely winning the radio race.
    let mut displayer = fresh_engine("Displayer");
    let mut harvester = fresh_engine("Harvester");
    let nonce = [9u8; 16];

    ensure_with_oob(
        &mut displayer,
        BleRole::Responder,
        Some(BleOobBinding {
            required_oob_nonce: Some(nonce),
            ..Default::default()
        }),
    );
    ensure_with_oob(&mut harvester, BleRole::Initiator, None);

    run_handshake(&mut harvester, &mut displayer);

    assert_eq!(
        displayer.vauchi.list_contacts().expect("contacts").len(),
        0,
        "displayer requires the QR nonce — a connector without the echo \
         must be rejected (no contact persisted)"
    );
}

// @scenario: ble_exchange :: Glance matching binding completes for both peers
#[test]
fn glance_matching_binding_completes_and_persists() {
    // Happy path: the scanner echoes the displayer's nonce and pins its
    // identity; the displayer requires that nonce. Both checks pass, the
    // exchange completes, and both persist the peer.
    let mut scanner = fresh_engine("Scanner");
    let mut displayer = fresh_engine("Displayer");
    let pinned = signing_key(&displayer);
    let nonce = [7u8; 16];

    ensure_with_oob(
        &mut scanner,
        BleRole::Initiator,
        Some(BleOobBinding {
            expected_peer: Some(pinned),
            oob_nonce_echo: Some(nonce),
            ..Default::default()
        }),
    );
    ensure_with_oob(
        &mut displayer,
        BleRole::Responder,
        Some(BleOobBinding {
            required_oob_nonce: Some(nonce),
            ..Default::default()
        }),
    );

    run_handshake(&mut scanner, &mut displayer);

    assert_eq!(
        scanner.vauchi.list_contacts().expect("contacts").len(),
        1,
        "scanner must persist the pinned displayer on success"
    );
    assert_eq!(
        displayer.vauchi.list_contacts().expect("contacts").len(),
        1,
        "displayer must persist the co-present scanner on success"
    );
}

// ============================================================
// Glance orchestration — scan → binding → gated discovery. The AppEngine
// computes the BleOobBinding from live QR state (the layer above the
// binding-threading tests: those inject the binding directly).
// ============================================================

// @scenario: ble_exchange :: Glance symmetric one-sided-QR completes for both peers
#[test]
fn glance_orchestration_symmetric_happy_path_both_persist() {
    // Symmetric UX: both devices display a QR + advertise + scan. Bob scans
    // Alice's QR (latching scanner), discovers Alice advertising, connects;
    // Alice (peripheral) responds. The pins are computed from the QR — no
    // binding is injected by hand.
    let mut alice = fresh_engine("Alice"); // displayer/responder
    let mut bob = fresh_engine("Bob"); // scanner/initiator

    let alice_qr = alice.begin_glance_display().expect("alice shows a QR");
    let _bob_qr = bob
        .begin_glance_display()
        .expect("bob also shows a QR (symmetric)");

    bob.apply_glance_scan(&alice_qr)
        .expect("bob scans alice's QR");
    let alice_id = signing_key(&alice);
    bob.handle_glance_discovery("alice-device", &alice_id);
    assert!(
        bob.ble_handshake_session_active(),
        "bob (scanner) builds an initiator session on discovering the scanned peer"
    );
    let connect: Vec<_> = bob
        .drain_pending_commands()
        .into_iter()
        .filter(|c| matches!(c, vauchi_core::Command::BleConnect { device_id } if device_id == "alice-device"))
        .collect();
    assert_eq!(
        connect.len(),
        1,
        "bob must emit exactly one BleConnect to the scanned peer"
    );

    alice.start_ble_handshake_as_responder();
    assert!(
        alice.ble_handshake_session_active(),
        "alice (displayer/peripheral) builds a responder session on connect"
    );

    run_handshake(&mut bob, &mut alice);

    assert_eq!(
        bob.vauchi.list_contacts().expect("contacts").len(),
        1,
        "scanner persists the displayer"
    );
    assert_eq!(
        alice.vauchi.list_contacts().expect("contacts").len(),
        1,
        "displayer persists the scanner"
    );
}

// @scenario: ble_exchange :: Glance scanner ignores an advertiser it did not scan (F1 dissolves)
#[test]
fn glance_orchestration_scanner_ignores_foreign_advertiser() {
    let mut alice = fresh_engine("Alice");
    let mut bob = fresh_engine("Bob");
    let alice_qr = alice.begin_glance_display().expect("alice QR");
    bob.apply_glance_scan(&alice_qr).expect("bob scans alice");

    let mallory = fresh_engine("Mallory");
    let mallory_id = signing_key(&mallory);
    bob.handle_glance_discovery("mallory-device", &mallory_id);

    assert!(
        !bob.ble_handshake_session_active(),
        "bob must not connect to an advertiser whose identity != the scanned QR"
    );
    assert!(
        bob.drain_pending_commands().is_empty(),
        "no BleConnect to a foreign advertiser (no latch race, F1 dissolves)"
    );
}

// @scenario: ble_exchange :: Glance identity-spoofing advertiser is rejected at the handshake pin
#[test]
fn glance_orchestration_identity_spoofing_advertiser_rejected_at_handshake() {
    // Mallory advertises Alice's (public) identity to satisfy bob's
    // discovery match, then answers with her own keys. The advertisement
    // match is NOT the security boundary — the session pin is.
    let mut alice = fresh_engine("Alice");
    let mut bob = fresh_engine("Bob");
    let mut mallory = fresh_engine("Mallory");

    let alice_qr = alice.begin_glance_display().expect("alice QR");
    bob.apply_glance_scan(&alice_qr).expect("bob scans alice");

    let alice_id = signing_key(&alice);
    bob.handle_glance_discovery("mallory-device", &alice_id);
    assert!(
        bob.ble_handshake_session_active(),
        "bob connects — the advertisement claimed alice's identity"
    );
    let _ = bob.drain_pending_commands();

    mallory.start_ble_handshake_as_responder();
    run_handshake(&mut bob, &mut mallory);

    assert_eq!(
        bob.vauchi.list_contacts().expect("contacts").len(),
        0,
        "the handshake pin rejects Mallory — she is not the scanned Alice"
    );
}
