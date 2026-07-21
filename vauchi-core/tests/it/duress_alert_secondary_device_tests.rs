// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! RED characterization: a coercion-safety / duress alert raised on a SECONDARY
//! linked device (one that joined via device-link and never did the QR exchange)
//! is silently dropped at send.
//!
//! Context — `backlog/2026-07-21-per-device-ratchet-registry-dormant` and
//! `problems/2026-07-10-multi-device-ratchet-topology-gap`. Per-device ratchet
//! sessions are keyed `(contact_id, peer_device_id)`, bootstrapped from a peer
//! `RegistryBroadcast`. That registry is persisted ONLY by the device-link path
//! (`DeviceSyncOrchestrator::apply_full_sync` -> `save_contact_device_registry`),
//! never from a contact exchange, and ratchet state itself is never synced
//! between an owner's devices (`api/sync/device_orchestrator.rs:388-390` "never a
//! live ratchet chain"). So a secondary device ends up holding the exchanged
//! Contact (with `shared_key` + `public_key`, copied by `DeviceSyncPayload`) but
//! NO ratchet row and NO peer registry for that contact.
//!
//! Card updates survive this via owner-device repropagation + idempotent LWW (a
//! secondary's edit syncs to the exchanged device, which repropagates it). A
//! one-shot safety/duress alert has no such repropagation path: it is built
//! per-recipient by `queue_safety_alerts` (`api/vauchi/emergency.rs:121`) through
//! `encrypt_payload_for_contact_devices`, which — with an empty registry and no
//! `[0;32]` ratchet — returns `Err(NotFound("ratchet state"))`
//! (`api/vauchi/propagation.rs:350`); `queue_safety_alerts` catches that as
//! `=> continue` (`emergency.rs:167`) and silently skips the recipient. Via
//! `authenticate()` the duress path additionally swallows the count
//! (`api/vauchi/security.rs:122-129`), so the coerced user gets zero signal the
//! alarm never sent.
//!
//! The two non-ignored tests are controls: an alert from a device WITH a ratchet
//! is delivered and received; a real owner-sync leaves the secondary device
//! without a ratchet or registry. The `#[ignore]`d tests assert the SAFE
//! behavior (the alert reaches the contact) and FAIL on current code — run them
//! with `--ignored`. Un-ignore when the dormant per-device registry is activated
//! (or the send path bootstraps a session from the synced `shared_key`).

use crate::common;

use common::device_sync::{
    create_test_contact, create_test_device, create_test_registry, create_test_storage,
};
use common::helpers::{create_vauchi_with_identity, setup_alice_bob_exchange, setup_ratchets};
use vauchi_core::api::sync::DeviceSyncOrchestrator;
use vauchi_core::api::{ReceiveOutcome, process_single_card_update};
use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::ratchet::DoubleRatchetState;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::sync::DeviceLinkIntent;
use vauchi_core::sync::safety_alert::AlertKind;
use vauchi_core::types::DuressSettings;
use vauchi_core::{AuthMode, SymmetricKey, Vauchi};

/// A secondary linked device's verified state after device-link + owner-device
/// sync: it holds the exchanged `Bob` contact (shared_key + public_key) but has
/// NO ratchet row and NO peer registry for Bob. Reproduced directly here; the
/// `real_owner_sync_leaves_secondary_device_without_ratchet_or_registry` control
/// below proves a genuine `DeviceSyncOrchestrator` sync produces exactly this
/// state. Returns `(secondary_device, bob_contact_id)`.
fn secondary_device_after_owner_sync() -> (Vauchi, String) {
    let secondary = create_vauchi_with_identity("Alice");
    let bob_pk = *create_vauchi_with_identity("Bob")
        .identity()
        .unwrap()
        .signing_public_key();
    let bob_contact =
        Contact::from_exchange(bob_pk, ContactCard::new("Bob"), SymmetricKey::generate(), 0);
    let bob_id = bob_contact.id().to_string();
    secondary.add_contact(bob_contact).unwrap();
    // Deliberately NO save_ratchet_state and NO save_contact_device_registry —
    // this is the exact end-state of a secondary device (see module docs).
    (secondary, bob_id)
}

// ---------------------------------------------------------------------------
// Controls (pass) — the harness works and the premise holds.
// ---------------------------------------------------------------------------

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// Control: an emergency alert from a device that DOES hold a ratchet is queued
/// and received. Isolates the secondary-device failure below as the real gap.
// @internal
#[test]
fn emergency_alert_with_established_ratchet_reaches_recipient() {
    let (mut alice_wb, bob_wb, secret, bob_id_at_alice, alice_id_at_bob) =
        setup_alice_bob_exchange();
    // Alice (initiator) is the sender; Bob (responder) receives — matching the
    // pairing setup_ratchets returns.
    let (alice_ratchet, bob_ratchet) = setup_ratchets(&secret);
    alice_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&bob_id_at_alice, &alice_ratchet, true)
        .unwrap();
    bob_wb
        .storage()
        .ratchets()
        .save_ratchet_state(&alice_id_at_bob, &bob_ratchet, false)
        .unwrap();

    alice_wb
        .configure_emergency_broadcast(vec![bob_id_at_alice.clone()], "check on me".into(), false)
        .unwrap();
    let result = alice_wb.send_emergency_broadcast().unwrap();
    assert_eq!(
        result.sent, 1,
        "a device with a ratchet must deliver the alert"
    );

    let pending = alice_wb
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    let blob = &pending
        .iter()
        .find(|u| u.contact_id == bob_id_at_alice)
        .expect("a queued alert for Bob")
        .payload;
    let outcome = process_single_card_update(
        bob_wb.identity().unwrap(),
        bob_wb.storage(),
        &alice_id_at_bob,
        blob,
    )
    .expect("the alert must be received");
    match outcome {
        ReceiveOutcome::Alert(a) => assert_eq!(a.kind, AlertKind::Emergency),
        other => panic!("expected an Emergency alert, got {other:?}"),
    }
}

/// Control (premise): a genuine `DeviceSyncOrchestrator` full-sync from an
/// exchanged device (A1) to a freshly linked device (A2) leaves A2 with the Bob
/// contact but ZERO ratchet rows and ZERO peer-registry rows for Bob. This is
/// the state the RED tests below rely on — proven here through the real module,
/// not assumed.
// @internal
#[test]
fn real_owner_sync_leaves_secondary_device_without_ratchet_or_registry() {
    let master_seed = [0x42u8; 32];

    // A1: exchanged with Bob — has the contact and a [0;32] session.
    let a1 = create_test_storage();
    let bob = create_test_contact("Bob");
    let bob_id = bob.id().to_string();
    a1.contacts().save_contact(&bob).unwrap();
    let dh = X3DHKeyPair::generate();
    let a1_ratchet =
        DoubleRatchetState::initialize_initiator(&SymmetricKey::generate(), *dh.public_key())
            .unwrap();
    a1.ratchets()
        .save_ratchet_state(&bob_id, &a1_ratchet, true)
        .unwrap();

    let a1_device = create_test_device(&master_seed, 0, "Device A");
    let a1_registry = create_test_registry(&master_seed, &a1_device);
    let payload = DeviceSyncOrchestrator::new(&a1, a1_device, a1_registry)
        .create_full_sync_payload(DeviceLinkIntent::AddDevice)
        .unwrap();
    assert!(
        payload.contact_device_registries.is_empty(),
        "a single-device peer has no registry to propagate"
    );

    // A2: freshly linked — applies A1's full sync.
    let a2 = create_test_storage();
    let a2_device = create_test_device(&master_seed, 1, "Device B");
    let a2_registry = create_test_registry(&master_seed, &a2_device);
    DeviceSyncOrchestrator::new(&a2, a2_device, a2_registry)
        .apply_full_sync(payload)
        .unwrap();

    assert!(
        a2.contacts().load_contact(&bob_id).unwrap().is_some(),
        "A2 must receive the Bob contact via owner-device sync"
    );
    assert!(
        a2.ratchets().load_ratchet_state(&bob_id).unwrap().is_none(),
        "ratchet state is never synced — A2 has no [0;32] session for Bob"
    );
    assert!(
        a2.device()
            .load_contact_active_devices(&bob_id)
            .unwrap()
            .is_empty(),
        "no peer registry is persisted for an exchanged single-device contact"
    );
}

// ---------------------------------------------------------------------------
// RED (ignored) — assert the SAFE behavior; fails on current code.
// ---------------------------------------------------------------------------

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// RED: a secondary device must be able to raise an emergency alert to an
/// exchanged contact. Current code returns `sent == 0` (silently skipped at
/// send). Un-ignore when the send path can establish a session for a
/// secondary device.
// @internal
#[test]
#[ignore = "RED: secondary-device alert silently dropped at send — backlog/2026-07-21-per-device-ratchet-registry-dormant"]
fn emergency_alert_from_secondary_device_should_reach_recipient() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary
        .configure_emergency_broadcast(vec![bob_id.clone()], "check on me".into(), false)
        .unwrap();
    let result = secondary.send_emergency_broadcast().unwrap();

    assert_eq!(
        result.sent, 1,
        "a secondary linked device must be able to raise an emergency alert"
    );
    let pending = secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert!(
        pending.iter().any(|u| u.contact_id == bob_id),
        "the emergency alert must be queued for the contact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// RED (most safety-critical): a duress unlock on a secondary device must queue
/// the disguised alert to the configured contact. Current code lets the unlock
/// succeed (`AuthMode::Duress`) while silently queuing nothing — the coerced
/// user gets no signal the alarm failed. Un-ignore when the send path can
/// establish a session for a secondary device.
// @internal
#[test]
#[ignore = "RED: secondary-device duress alert silently not queued — backlog/2026-07-21-per-device-ratchet-registry-dormant"]
fn duress_unlock_from_secondary_device_should_queue_alert() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary.setup_app_password("normal-pin-1234").unwrap();
    secondary.setup_duress_password("duress-pin-9876").unwrap();
    secondary
        .save_duress_settings(&DuressSettings {
            alert_contact_ids: vec![bob_id.clone()],
            alert_message: "I need help".to_string(),
            include_location: false,
        })
        .unwrap();

    let mode = secondary
        .authenticate("duress-pin-9876")
        .expect("duress unlock must succeed");
    assert_eq!(mode, AuthMode::Duress);

    let pending = secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert!(
        pending.iter().any(|u| u.contact_id == bob_id),
        "the duress unlock must queue a disguised alert for the configured contact"
    );
}

// @scenario: duress_mode :: Duress unlock sends silent alert to trusted contacts
/// RED (end-to-end): the recipient must surface a secondary device's emergency
/// alert. Current code queues nothing at the sender, so the recipient never
/// receives it. Un-ignore when the send path can establish a session for a
/// secondary device.
// @internal
#[test]
#[ignore = "RED: recipient never surfaces a secondary-device alert — backlog/2026-07-21-per-device-ratchet-registry-dormant"]
fn secondary_device_alert_should_be_surfaced_by_recipient() {
    let (secondary, bob_id) = secondary_device_after_owner_sync();
    let mut secondary = secondary;

    secondary
        .configure_emergency_broadcast(vec![bob_id.clone()], "check on me".into(), false)
        .unwrap();
    secondary.send_emergency_broadcast().unwrap();

    let pending = secondary
        .storage()
        .pending()
        .get_all_pending_updates()
        .unwrap();
    assert!(
        pending.iter().any(|u| u.contact_id == bob_id),
        "a secondary-device alert must reach the wire so the recipient can surface it"
    );
}
