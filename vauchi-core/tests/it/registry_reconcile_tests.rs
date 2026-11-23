// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! This identity's devices converge on one registry whatever order they
//! change it and hear from each other in (private#970, owner 2026-11-23).
//! Devices are never removed from a registry, only revoked, so a sibling
//! merges what it receives with what it holds: every device, every named
//! key, every revocation, the same merge in either order.

use std::collections::BTreeSet;

use proptest::prelude::*;
use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry, Reconciled, RegisteredDevice};

const SEED: [u8; 32] = [5u8; 32];
const SIBLINGS: usize = 3;
const NOW: u64 = 1_700_000_000;

fn identity_key() -> SigningKeyPair {
    SigningKeyPair::from_seed(&SEED)
}

fn device(index: u32) -> RegisteredDevice {
    DeviceInfo::derive(&SEED, index, format!("device {index}"), u64::from(index))
        .to_registered(&SEED)
}

fn device_id(index: u32) -> [u8; 32] {
    device(index).device_id
}

/// The registry three linked siblings share before any names a key.
fn shared_registry() -> DeviceRegistry {
    let key = identity_key();
    let mut registry = DeviceRegistry::new(device(0), &key);
    for index in 1..SIBLINGS as u32 {
        registry.add_device(device(index), &key).unwrap();
    }
    registry
}

/// What the identity signs, in order: the version and each device's id,
/// exchange key, signing key and revocation.
type Signed = (u64, Vec<([u8; 32], [u8; 32], Option<[u8; 32]>, bool)>);

fn signed(registry: &DeviceRegistry) -> Signed {
    (
        registry.version(),
        registry
            .all_devices()
            .iter()
            .map(|d| {
                (
                    d.device_id,
                    d.exchange_public_key,
                    d.signing_public_key,
                    d.revoked,
                )
            })
            .collect(),
    )
}

/// `receiver` applies `incoming` the way a sibling applies an owner-sync
/// registry.
fn deliver(receiver: &DeviceRegistry, incoming: &DeviceRegistry) -> DeviceRegistry {
    match receiver.reconcile(incoming, &identity_key()).unwrap() {
        Reconciled::Keep => receiver.clone(),
        Reconciled::Adopt => incoming.clone(),
        Reconciled::Publish(merged) => merged,
    }
}

fn named(registry: &DeviceRegistry, index: u32, key: &SigningKeyPair) -> DeviceRegistry {
    let mut registry = registry.clone();
    registry
        .name_device_key(
            &device_id(index),
            *key.public_key().as_bytes(),
            &identity_key(),
        )
        .unwrap();
    registry
}

fn key_of(registry: &DeviceRegistry, index: u32) -> Option<[u8; 32]> {
    registry
        .find_device(&device_id(index))
        .and_then(|d| d.signing_public_key)
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_registry_reconciled_with_itself_is_kept() {
    let registry = shared_registry();

    assert!(matches!(
        registry.reconcile(&registry, &identity_key()).unwrap(),
        Reconciled::Keep
    ));
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn two_siblings_naming_their_keys_at_one_version_publish_the_same_merge() {
    let (phone_key, laptop_key) = (SigningKeyPair::generate(), SigningKeyPair::generate());
    let phone = named(&shared_registry(), 0, &phone_key);
    let laptop = named(&shared_registry(), 1, &laptop_key);
    assert_eq!(phone.version(), laptop.version());

    let at_phone = deliver(&phone, &laptop);
    let at_laptop = deliver(&laptop, &phone);

    assert_eq!(signed(&at_phone), signed(&at_laptop));
    assert!(at_phone.version() > phone.version());
    assert_eq!(
        key_of(&at_phone, 0),
        Some(*phone_key.public_key().as_bytes())
    );
    assert_eq!(
        key_of(&at_phone, 1),
        Some(*laptop_key.public_key().as_bytes())
    );
    assert!(at_phone.verify(&identity_key().public_key()));
}

// @scenario: device_management :: Device-specific keys
// @internal
#[test]
fn a_newer_registry_without_the_key_this_device_named_does_not_drop_it() {
    let phone_key = SigningKeyPair::generate();
    let phone = named(&shared_registry(), 0, &phone_key);
    let mut laptop = shared_registry();
    laptop
        .revoke_device(&device_id(2), &identity_key(), NOW)
        .unwrap();
    laptop.lift_version_to(phone.version() + 5, &identity_key());

    let at_phone = deliver(&phone, &laptop);

    assert_eq!(
        key_of(&at_phone, 0),
        Some(*phone_key.public_key().as_bytes())
    );
    assert!(at_phone.find_device(&device_id(2)).unwrap().revoked);
    assert!(at_phone.version() > laptop.version());
}

#[derive(Debug, Clone)]
enum Op {
    NameOwnKey(usize),
    Link(usize, u32),
    Revoke(usize, u32),
    MakePrimary(usize, u32),
    Deliver(usize, usize),
}

fn op() -> impl Strategy<Value = Op> {
    let sibling = 0..SIBLINGS;
    prop_oneof![
        sibling.clone().prop_map(Op::NameOwnKey),
        (sibling.clone(), 10u32..13).prop_map(|(s, d)| Op::Link(s, d)),
        (sibling.clone(), 0u32..13).prop_map(|(s, d)| Op::Revoke(s, d)),
        (sibling.clone(), 0u32..13).prop_map(|(s, d)| Op::MakePrimary(s, d)),
        (sibling.clone(), sibling).prop_map(|(from, to)| Op::Deliver(from, to)),
    ]
}

proptest! {
    // Siblings change their registries and hear from each other in any
    // order; once every sibling has heard from every other, they hold one
    // registry, and it lost nothing any of them did.
    // @scenario: device_management :: Device-specific keys
    // @internal
    #[test]
    fn siblings_converge_on_one_registry_that_loses_nothing(
        ops in proptest::collection::vec(op(), 0..40)
    ) {
        let identity = identity_key();
        let keys: Vec<SigningKeyPair> = (0..SIBLINGS).map(|_| SigningKeyPair::generate()).collect();
        let mut held: Vec<DeviceRegistry> = vec![shared_registry(); SIBLINGS];
        let mut named_by = BTreeSet::new();
        let mut linked = BTreeSet::new();
        let mut revoked = BTreeSet::new();

        for op in ops {
            match op {
                Op::NameOwnKey(s) => {
                    let mut next = held[s].clone();
                    if next
                        .name_device_key(&device_id(s as u32), *keys[s].public_key().as_bytes(), &identity)
                        .is_ok()
                    {
                        held[s] = next;
                        named_by.insert(s);
                    }
                }
                Op::Link(s, index) => {
                    let mut next = held[s].clone();
                    if next.add_device(device(index), &identity).is_ok() {
                        held[s] = next;
                        linked.insert(index);
                    }
                }
                Op::Revoke(s, index) => {
                    let mut next = held[s].clone();
                    if index != s as u32
                        && next.revoke_device(&device_id(index), &identity, NOW).is_ok()
                    {
                        held[s] = next;
                        revoked.insert(index);
                    }
                }
                Op::MakePrimary(s, index) => {
                    let mut next = held[s].clone();
                    if next.transfer_primary(&device_id(index), &identity).is_ok() {
                        held[s] = next;
                    }
                }
                Op::Deliver(from, to) if from != to => {
                    held[to] = deliver(&held[to], &held[from]);
                }
                Op::Deliver(..) => {}
            }
        }

        let mut rounds = 0;
        loop {
            let before: Vec<Signed> = held.iter().map(signed).collect();
            for from in 0..SIBLINGS {
                for to in 0..SIBLINGS {
                    if from != to {
                        held[to] = deliver(&held[to], &held[from]);
                    }
                }
            }
            let after: Vec<Signed> = held.iter().map(signed).collect();
            rounds += 1;
            if before == after {
                break;
            }
            prop_assert!(rounds < 10, "no convergence after {rounds} rounds");
        }

        for other in &held[1..] {
            prop_assert_eq!(signed(other), signed(&held[0]));
        }
        let one = &held[0];
        prop_assert!(one.verify(&identity.public_key()));
        for s in named_by {
            prop_assert_eq!(key_of(one, s as u32), Some(*keys[s].public_key().as_bytes()));
        }
        for index in linked {
            prop_assert!(one.find_device(&device_id(index)).is_some(), "linked device {} lost", index);
        }
        for index in revoked {
            prop_assert!(one.find_device(&device_id(index)).unwrap().revoked, "revocation of {} lost", index);
        }
    }
}
