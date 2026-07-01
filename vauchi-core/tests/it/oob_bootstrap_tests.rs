// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! OOB bootstrap payload (Glance one-sided QR) — codec + co-presence nonce.
//!
//! Tier 1 Slice B of `2026-06-10-ble-unauthenticated-peer-identity`.
//! The QR carries the fixed 174-byte `ExchangeBle` shape under a distinct
//! magic; the 16-byte OOB co-presence nonce is HKDF-derived from the signed
//! `token` so it inherits the identity signature and — because the token is
//! never shipped over BLE in the handshake flow — stays a QR-exclusive secret
//! (ADR-053). These tests pin: codec roundtrip, nonce determinism (so the
//! displayer's `required_oob_nonce` equals the scanner's echoed nonce for the
//! same QR), signature/expiry enforcement, and adversarial rejection (CC-14).

use proptest::prelude::*;
use vauchi_core::exchange::X3DHKeyPair;
use vauchi_core::exchange::oob_bootstrap::{
    OOB_EXPIRY_SECONDS, OOB_NONCE_SIZE, OobBootstrapQr, derive_oob_nonce,
};
use vauchi_core::identity::Identity;

fn displayer() -> (Identity, X3DHKeyPair) {
    (Identity::create("Displayer", 0), X3DHKeyPair::generate())
}

// @internal
#[test]
fn oob_qr_roundtrip_preserves_identity_exchange_and_nonce() {
    let (id, eph) = displayer();
    let now = 1_000;
    let qr = OobBootstrapQr::generate(&id, &eph, now);

    let encoded = qr.to_data_string();
    let parsed = OobBootstrapQr::from_data_string(&encoded).expect("valid QR must parse");

    assert_eq!(
        parsed.identity_key(),
        id.signing_public_key(),
        "identity pin material must survive the roundtrip"
    );
    assert_eq!(parsed.exchange_key(), eph.public_key());
    assert_eq!(parsed.token(), qr.token());
    assert_eq!(
        parsed.oob_nonce(),
        qr.oob_nonce(),
        "the scanner must derive the same nonce the displayer requires"
    );
    assert!(
        parsed.verify_signature(),
        "signature must verify after roundtrip"
    );
    assert!(
        !parsed.is_expired(now + OOB_EXPIRY_SECONDS),
        "payload must be fresh at the window edge"
    );
}

// @internal
#[test]
fn oob_nonce_is_deterministic_function_of_token() {
    let token = [7u8; 32];
    assert_eq!(
        derive_oob_nonce(&token),
        derive_oob_nonce(&token),
        "same token must derive the same 16-byte nonce (displayer==scanner echo)"
    );
    assert_eq!(derive_oob_nonce(&token).len(), OOB_NONCE_SIZE);
}

// @internal
#[test]
fn oob_nonce_is_hkdf_derived_not_a_token_slice() {
    // Non-triviality: the nonce must be a domain-separated HKDF of the token,
    // not a raw slice — otherwise a future codec change that exposed the token
    // prefix over the radio would silently expose the nonce too.
    let token = [0x5A; 32];
    let nonce = derive_oob_nonce(&token);
    let prefix: [u8; OOB_NONCE_SIZE] = token[..OOB_NONCE_SIZE].try_into().unwrap();
    assert_ne!(
        nonce, prefix,
        "nonce must not equal the token's leading bytes"
    );
    let suffix: [u8; OOB_NONCE_SIZE] = token[16..32].try_into().unwrap();
    assert_ne!(
        nonce, suffix,
        "nonce must not equal the token's trailing bytes"
    );
}

// @internal
#[test]
fn oob_nonce_differs_for_distinct_known_tokens() {
    assert_ne!(
        derive_oob_nonce(&[1u8; 32]),
        derive_oob_nonce(&[2u8; 32]),
        "distinct tokens must derive distinct nonces"
    );
}

// @internal
#[test]
fn oob_qr_tampered_payload_fails_signature() {
    let (id, eph) = displayer();
    let qr = OobBootstrapQr::generate(&id, &eph, 1_000);
    let mut bytes = qr.to_bytes();
    bytes[6] ^= 0xFF; // flip an identity-key byte inside the signed region

    let parsed =
        OobBootstrapQr::from_bytes(&bytes).expect("magic/version/length still valid → parses");
    assert!(
        !parsed.verify_signature(),
        "a tampered payload must fail the Ed25519 check"
    );
}

// @internal
#[test]
fn oob_qr_expiry_boundary() {
    let (id, eph) = displayer();
    let qr = OobBootstrapQr::generate_with_timestamp(&id, &eph, [3u8; 32], 1_000);
    assert!(
        !qr.is_expired(1_000 + OOB_EXPIRY_SECONDS),
        "not expired at the exact window edge"
    );
    assert!(
        qr.is_expired(1_000 + OOB_EXPIRY_SECONDS + 1),
        "expired one second past the window"
    );
}

// @internal
#[test]
fn oob_qr_rejects_wrong_magic_and_truncation() {
    let (id, eph) = displayer();
    let mut bytes = OobBootstrapQr::generate(&id, &eph, 0).to_bytes();
    bytes[0] = b'X';
    assert!(
        OobBootstrapQr::from_bytes(&bytes).is_err(),
        "a foreign magic must be rejected"
    );
    assert!(
        OobBootstrapQr::from_bytes(&[0u8; 10]).is_err(),
        "a truncated payload must be rejected"
    );
}

// @internal
#[test]
fn oob_qr_rejects_oversized_payload_with_trailing_bytes() {
    // Encoding malleability: bytes past the fixed 174 are outside the signed
    // region; accepting them would give one logical QR unbounded valid encodings.
    let (id, eph) = displayer();
    let mut oversized = OobBootstrapQr::generate(&id, &eph, 0).to_bytes().to_vec();
    oversized.extend_from_slice(&[0xAB; 8]);
    assert!(
        OobBootstrapQr::from_bytes(&oversized).is_err(),
        "a payload with trailing bytes must be rejected, not silently truncated"
    );
}

// @internal
#[test]
fn verified_from_data_string_enforces_signature_and_expiry() {
    let (id, eph) = displayer();
    let qr = OobBootstrapQr::generate_with_timestamp(&id, &eph, [9u8; 32], 1_000);
    let encoded = qr.to_data_string();

    let ok = OobBootstrapQr::verified_from_data_string(&encoded, 1_000)
        .expect("fresh, correctly-signed QR is accepted");
    assert_eq!(ok.identity_key(), id.signing_public_key());

    assert!(
        OobBootstrapQr::verified_from_data_string(&encoded, 1_000 + OOB_EXPIRY_SECONDS + 1)
            .is_err(),
        "an expired QR must be rejected by the verified constructor"
    );

    let mut tampered = qr.to_bytes();
    tampered[38] ^= 0x01; // flip an exchange-key byte (signed region)
    let tampered_encoded = OobBootstrapQr::from_bytes(&tampered)
        .expect("parses")
        .to_data_string();
    assert!(
        OobBootstrapQr::verified_from_data_string(&tampered_encoded, 1_000).is_err(),
        "a tampered QR must be rejected by the verified constructor"
    );
}

proptest! {
    // @internal
    #[test]
    fn oob_nonce_derivation_is_stable(token in any::<[u8; 32]>()) {
        prop_assert_eq!(derive_oob_nonce(&token), derive_oob_nonce(&token));
    }

    // @internal
    #[test]
    fn oob_qr_roundtrips_for_arbitrary_token_and_time(
        token in any::<[u8; 32]>(),
        ts in 0u64..u64::MAX / 2,
    ) {
        let id = Identity::create("D", 0);
        let eph = X3DHKeyPair::generate();
        let qr = OobBootstrapQr::generate_with_timestamp(&id, &eph, token, ts);
        let parsed = OobBootstrapQr::from_data_string(&qr.to_data_string())
            .expect("roundtrip must parse");
        prop_assert_eq!(parsed.token(), &token);
        prop_assert_eq!(parsed.oob_nonce(), qr.oob_nonce());
        prop_assert!(parsed.verify_signature());
    }
}
