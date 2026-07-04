// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Signed safety-alert payload envelope (VersionedPayload 0x04).
//!
//! Crypto foundation for the coercion-safety alert receive path
//! (`2026-07-04-coercion-safety-alerts-never-received`). Verifies the
//! security-review decisions: signed (sender+recipient binding), replay
//! nonce, strict version dispatch (no misdetection).

use vauchi_core::identity::Identity;
use vauchi_core::network::GeoLocation;
use vauchi_core::sync::delta::{PAYLOAD_VERSION_ALERT, VersionedPayload};
use vauchi_core::sync::safety_alert::{AlertKind, SafetyAlertPayload};

fn keypairs() -> (Identity, [u8; 32], Identity, [u8; 32]) {
    let sender = Identity::create("Sender", 0);
    let recipient = Identity::create("Recipient", 1);
    let sender_pk = *sender.signing_public_key();
    let recipient_pk = *recipient.signing_public_key();
    (sender, sender_pk, recipient, recipient_pk)
}

fn sample_location() -> GeoLocation {
    GeoLocation {
        latitude: 47.3769,
        longitude: 8.5417,
        accuracy_meters: Some(12.0),
    }
}

// @internal
#[test]
fn roundtrip_encode_decode_preserves_content_and_verifies() {
    let (sender, sender_pk, _r, recipient_pk) = keypairs();
    let payload = SafetyAlertPayload::new(
        AlertKind::Duress,
        "help".into(),
        1_720_000_000,
        Some(sample_location()),
        [7u8; 32],
        &sender,
        &recipient_pk,
    )
    .expect("build alert");

    let decoded = SafetyAlertPayload::decode(&payload.encode()).expect("decode");
    assert_eq!(decoded.kind(), AlertKind::Duress);
    assert_eq!(decoded.message(), "help");
    assert_eq!(decoded.timestamp(), 1_720_000_000);
    assert_eq!(decoded.location(), Some(&sample_location()));
    assert_eq!(decoded.nonce(), &[7u8; 32]);
    assert!(
        decoded.verify(&sender_pk, &recipient_pk),
        "a correctly-signed alert must verify"
    );
}

// @internal
#[test]
fn both_kinds_roundtrip() {
    let (sender, sender_pk, _r, recipient_pk) = keypairs();
    for kind in [AlertKind::Emergency, AlertKind::Duress] {
        let p =
            SafetyAlertPayload::new(kind, "m".into(), 1, None, [1u8; 32], &sender, &recipient_pk)
                .unwrap();
        let d = SafetyAlertPayload::decode(&p.encode()).unwrap();
        assert_eq!(d.kind(), kind);
        assert!(d.verify(&sender_pk, &recipient_pk));
    }
}

// @internal
#[test]
fn tampered_content_fails_verification() {
    let (sender, sender_pk, _r, recipient_pk) = keypairs();
    let payload = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "original".into(),
        1,
        None,
        [2u8; 32],
        &sender,
        &recipient_pk,
    )
    .unwrap();
    let mut wire = payload.encode();
    let last = wire.len() - 1;
    wire[last] ^= 0xFF;
    if let Ok(d) = SafetyAlertPayload::decode(&wire) {
        assert!(
            !d.verify(&sender_pk, &recipient_pk),
            "tampered content must not verify"
        );
    }
}

// @internal
#[test]
fn wrong_sender_key_fails_verification() {
    let (sender, _sender_pk, _r, recipient_pk) = keypairs();
    let attacker_pk = *Identity::create("Mallory", 9).signing_public_key();
    let payload = SafetyAlertPayload::new(
        AlertKind::Duress,
        "m".into(),
        1,
        None,
        [3u8; 32],
        &sender,
        &recipient_pk,
    )
    .unwrap();
    assert!(
        !payload.verify(&attacker_pk, &recipient_pk),
        "verifying against a different sender key must fail"
    );
}

// @internal
#[test]
fn wrong_recipient_key_fails_verification() {
    let (sender, sender_pk, _r, recipient_pk) = keypairs();
    let other_recipient = *Identity::create("Other", 8).signing_public_key();
    let payload = SafetyAlertPayload::new(
        AlertKind::Duress,
        "m".into(),
        1,
        None,
        [4u8; 32],
        &sender,
        &recipient_pk,
    )
    .unwrap();
    assert!(
        !payload.verify(&sender_pk, &other_recipient),
        "an alert signed for one recipient must not verify for another (no cross-recipient replay)"
    );
}

// @internal
#[test]
fn decode_rejects_truncated_wire() {
    assert!(
        SafetyAlertPayload::decode(&[0u8; 40]).is_err(),
        "a wire shorter than the prefix must be rejected"
    );
}

// @internal
#[test]
fn versioned_payload_roundtrips_via_alert_variant() {
    let (sender, sender_pk, _r, recipient_pk) = keypairs();
    let payload = SafetyAlertPayload::new(
        AlertKind::Emergency,
        "m".into(),
        1,
        None,
        [5u8; 32],
        &sender,
        &recipient_pk,
    )
    .unwrap();
    let wire = VersionedPayload::encode_alert(&payload);
    assert_eq!(wire[0], PAYLOAD_VERSION_ALERT, "version byte prefix");
    match VersionedPayload::decode(&wire).expect("decode versioned") {
        VersionedPayload::Alert(a) => {
            assert!(a.verify(&sender_pk, &recipient_pk));
            assert_eq!(a.message(), "m");
        }
        other => panic!("expected Alert variant, got {other:?}"),
    }
}

// @internal
#[test]
fn versioned_payload_rejects_unknown_version() {
    // 0x06 is not a known payload version — strict dispatch, no fallback,
    // so an alert can never be misrouted to a card delta or vice versa.
    assert!(
        VersionedPayload::decode(&[0x06, 1, 2, 3]).is_err(),
        "unknown version byte must be rejected"
    );
}
