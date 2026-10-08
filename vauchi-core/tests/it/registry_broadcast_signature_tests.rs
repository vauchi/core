// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A registry broadcast's signature covers its contents, so a contact
//! rejects one whose fields were changed after signing
//! (vauchi/private#522).

use vauchi_core::Identity;
use vauchi_core::identity::RegistryBroadcast;

fn tampered(broadcast: &RegistryBroadcast, field: &str, value: u64) -> RegistryBroadcast {
    let mut json = serde_json::to_value(broadcast).unwrap();
    json[field] = value.into();
    serde_json::from_value(json).unwrap()
}

// @internal
#[test]
fn a_broadcast_changed_after_signing_does_not_verify() {
    let alice = Identity::create("Alice", 0);
    let registry = alice.initial_device_registry();
    let broadcast = RegistryBroadcast::new(&registry, alice.signing_keypair(), 1_000);
    let key = &vauchi_core::crypto::PublicKey::from_bytes(*alice.signing_public_key());

    assert!(broadcast.verify(key));
    assert!(!tampered(&broadcast, "timestamp", 2_000).verify(key));
    assert!(!tampered(&broadcast, "version", broadcast.version() + 1).verify(key));
}

// A broadcast is fresh from `max_age` seconds old up to 60 seconds in the
// future (clock skew), and stale one second beyond either edge.
// @internal
#[test]
fn a_broadcast_is_fresh_within_its_age_and_skew_windows() {
    let alice = Identity::create("Alice", 0);
    let key = vauchi_core::crypto::PublicKey::from_bytes(*alice.signing_public_key());
    let stamped = 10_000;
    let broadcast = RegistryBroadcast::new(
        &alice.initial_device_registry(),
        alice.signing_keypair(),
        stamped,
    );
    let fresh_at = |now| broadcast.verify_with_freshness(&key, 0, now, 300).is_ok();

    assert_eq!(broadcast.timestamp(), stamped);
    assert!(fresh_at(stamped));
    assert!(fresh_at(stamped + 300));
    assert!(!fresh_at(stamped + 301));
    assert!(fresh_at(stamped - 60));
    assert!(!fresh_at(stamped - 61));
}
