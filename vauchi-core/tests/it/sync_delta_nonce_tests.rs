// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A card delta's replay nonce is required and never all-zero
//! (vauchi/private#568).

use proptest::prelude::*;
use serde_json::Value;
use vauchi_core::ContactCard;
use vauchi_core::sync::CardDelta;

fn delta_json() -> Value {
    let delta = CardDelta::compute(&ContactCard::new("Alice"), &ContactCard::new("Alicia"), 0);
    serde_json::to_value(&delta).unwrap()
}

fn base64(bytes: &[u8]) -> Value {
    Value::String(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        bytes,
    ))
}

// @internal
#[test]
fn a_delta_without_a_nonce_does_not_parse() {
    let mut json = delta_json();
    json.as_object_mut().unwrap().remove("nonce");

    let err = serde_json::from_value::<CardDelta>(json).unwrap_err();

    assert!(err.to_string().contains("nonce"), "unexpected error: {err}");
}

// @internal
#[test]
fn a_delta_with_an_all_zero_nonce_does_not_parse() {
    let mut json = delta_json();
    json["nonce"] = base64(&[0u8; 32]);

    let err = serde_json::from_value::<CardDelta>(json).unwrap_err();

    assert!(err.to_string().contains("nonce"), "unexpected error: {err}");
}

proptest! {
    // @internal
    #[test]
    fn any_nonzero_nonce_round_trips(nonce in any::<[u8; 32]>().prop_filter("non-zero", |n| n != &[0u8; 32])) {
        let mut json = delta_json();
        json["nonce"] = base64(&nonce);

        let delta: CardDelta = serde_json::from_value(json).unwrap();

        prop_assert_eq!(delta.nonce, nonce);
    }
}
