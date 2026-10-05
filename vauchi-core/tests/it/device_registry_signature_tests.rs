// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A device registry's signature covers its devices: editing the stored
//! registry after signing breaks verification (vauchi/private#522).

use vauchi_core::crypto::SigningKeyPair;
use vauchi_core::identity::{DeviceInfo, DeviceRegistry};

const SEED: [u8; 32] = [9u8; 32];

fn signed_registry() -> (DeviceRegistry, SigningKeyPair) {
    let signing = SigningKeyPair::from_seed(&SEED);
    let registry = DeviceRegistry::new(
        DeviceInfo::derive(&SEED, 0, "phone".into(), 0).to_registered(&SEED),
        &signing,
    );
    (registry, signing)
}

// @internal
#[test]
fn a_registry_verifies_against_its_signer_only() {
    let (registry, signing) = signed_registry();

    assert!(registry.verify(&signing.public_key()));
    assert!(!registry.verify(&SigningKeyPair::from_seed(&[1u8; 32]).public_key()));
}

// @internal
#[test]
fn editing_a_signed_registry_breaks_its_signature() {
    let (registry, signing) = signed_registry();
    let json = registry.to_json();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    value["devices"][0]["revoked"] = serde_json::Value::Bool(true);

    let tampered = DeviceRegistry::from_json(&value.to_string()).unwrap();

    assert!(!tampered.verify(&signing.public_key()));
}
