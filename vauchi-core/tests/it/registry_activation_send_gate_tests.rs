// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! F4 send gate: per-device fan-out requires `Active`, never registry
//! presence alone (ADR-064 Amendment 2026-07-25).
//!
//! Holding a contact's registry proves nothing about the *peer's* ability to
//! resolve our device-scoped tokens — switching on presence alone is the
//! B-lite hazard the 2026-07-24 discourse refuted (the peer would receive
//! sends it cannot resolve). Only a completed bilateral handshake
//! (`ActivationState::Active`) may open the per-device path; everything else
//! stays on the legacy `[0;32]` session the peer is guaranteed to hold.

use vauchi_core::contact_card::ContactCard;
use vauchi_core::exchange::{X3DH, X3DHKeyPair};
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, RegistryBroadcast};
use vauchi_core::sync::registry_activation::ActivationTracker;
use vauchi_core::{Contact, ContactField, FieldType, SigningKeyPair, SymmetricKey, Vauchi};

fn setup_with_card(name: &str) -> Vauchi {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity(name).unwrap();
    let field = ContactField::new(FieldType::Email, "Work", "alice@example.com", 0);
    let field_id = field.id().to_string();
    wb.add_own_field(field).unwrap();
    wb.set_own_field_public(&field_id).unwrap();
    wb
}

struct SeededPeer {
    contact_id: String,
}

/// Contact with an established legacy `[0;32]` ratchet AND a stored
/// two-device registry — the exact state where the pre-F4 predicate
/// (registry presence) and the F4 predicate (Active) disagree.
fn contact_with_registry_and_legacy_ratchet(wb: &Vauchi) -> SeededPeer {
    let alice_identity = wb.identity().unwrap();
    let signing = (1u32..=100_000)
        .map(|value| {
            let mut seed = [0u8; 32];
            seed[..4].copy_from_slice(&value.to_le_bytes());
            SigningKeyPair::from_seed(&seed)
        })
        .find(|candidate| candidate.public_key().as_bytes() > alice_identity.signing_public_key())
        .expect("a lexicographically larger test identity");

    let alice_x3dh = alice_identity.x3dh_keypair();
    let bob_x3dh = X3DHKeyPair::generate();
    let (shared_secret, _) = X3DH::initiate(&alice_x3dh, bob_x3dh.public_key()).unwrap();

    let contact = Contact::from_exchange(
        *signing.public_key().as_bytes(),
        ContactCard::new("Bob"),
        shared_secret.clone(),
        0,
    );
    let contact_id = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    wb.create_ratchet_as_initiator(&contact_id, &shared_secret, *bob_x3dh.public_key())
        .unwrap();

    let device_seed = [44u8; 32];
    let first = DeviceInfo::derive(&device_seed, 0, "Bob phone".into(), 1);
    let second = DeviceInfo::derive(&device_seed, 1, "Bob laptop".into(), 1);
    let mut registry = DeviceRegistry::new(first.to_registered(&device_seed), &signing);
    registry
        .add_device(second.to_registered(&device_seed), &signing)
        .unwrap();
    let now = wb.storage().clock().unix_seconds();
    let broadcast = RegistryBroadcast::new(&registry, &signing, now);
    wb.storage()
        .device()
        .save_contact_device_registry(&contact_id, &broadcast, signing.public_key().as_bytes(), 60)
        .unwrap();

    SeededPeer { contact_id }
}

fn prepare_updates(wb: &Vauchi, contact_id: &str) -> Vec<([u8; 32], Vec<u8>)> {
    let empty = ContactCard::new("Alice");
    let current = wb.storage().contacts().load_own_card().unwrap().unwrap();
    wb.prepare_card_updates_for_contact(contact_id, &empty, &current)
        .unwrap()
}

// @scenario: multi_device_sync :: Registry presence alone never switches sends off the legacy session
// @internal
#[test]
fn registry_presence_without_activation_keeps_the_legacy_send_path() {
    let wb = setup_with_card("Alice");
    let peer = contact_with_registry_and_legacy_ratchet(&wb);

    let updates = prepare_updates(&wb, &peer.contact_id);

    assert_eq!(
        updates.len(),
        1,
        "un-activated contact must get exactly one legacy copy, not a fan-out"
    );
    assert_eq!(
        updates[0].0, [0u8; 32],
        "the single copy rides the legacy all-zero session"
    );
}

// @scenario: multi_device_sync :: A confirmed handshake opens per-device fan-out
// @internal
#[test]
fn active_state_opens_per_device_fan_out() {
    let wb = setup_with_card("Alice");
    let peer = contact_with_registry_and_legacy_ratchet(&wb);

    // Simulate a completed bilateral handshake (push acked by the peer).
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([7u8; 32], 1);
    tracker.record_ack(&[7u8; 32], 1).unwrap();
    wb.storage()
        .registry_activation()
        .save_activation(&peer.contact_id, &tracker)
        .unwrap();

    let updates = prepare_updates(&wb, &peer.contact_id);

    assert_eq!(updates.len(), 2, "Active contact fans out per device");
    assert_ne!(updates[0].0, updates[1].0);
    assert!(
        updates.iter().all(|(device_id, _)| *device_id != [0u8; 32]),
        "no copy rides the legacy session once per-device is active"
    );
}

// @scenario: multi_device_sync :: Demoted activation falls back to the legacy session
// @internal
#[test]
fn demoted_activation_returns_to_the_legacy_send_path() {
    let wb = setup_with_card("Alice");
    let peer = contact_with_registry_and_legacy_ratchet(&wb);

    // Handshake completed, then our registry changed (re-push outstanding,
    // not yet re-acked) — sends must drop back to the safe legacy path.
    let mut tracker = ActivationTracker::new();
    tracker.record_push_sent([7u8; 32], 1);
    tracker.record_ack(&[7u8; 32], 1).unwrap();
    tracker.record_push_sent([8u8; 32], 2);
    wb.storage()
        .registry_activation()
        .save_activation(&peer.contact_id, &tracker)
        .unwrap();

    let updates = prepare_updates(&wb, &peer.contact_id);

    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].0, [0u8; 32]);
}
