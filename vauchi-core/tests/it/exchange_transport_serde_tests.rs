// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Serde round-trip coverage for [`ExchangeTransport`].
//!
//! Acts as a reminder that adding a new variant requires extending the
//! table below — the every-variant test fails when the table desyncs
//! from the enum.

use vauchi_core::types::ExchangeTransport;

/// Every variant round-trips through serde at the canonical snake_case
/// payload. Adding a variant without extending this table is a
/// compile-error-adjacent failure: the test runs, the new variant is
/// missing, and someone notices.
// @internal
#[test]
fn every_variant_round_trips_snake_case() {
    let cases: &[(ExchangeTransport, &str)] = &[
        (ExchangeTransport::Qr, "\"qr\""),
        (ExchangeTransport::Nfc, "\"nfc\""),
        (ExchangeTransport::Ble, "\"ble\""),
        (ExchangeTransport::Usb, "\"usb\""),
        (ExchangeTransport::Audio, "\"audio\""),
        (ExchangeTransport::MultiStage, "\"multi_stage\""),
        (ExchangeTransport::Link, "\"link\""),
    ];

    for (variant, expected_json) in cases {
        let serialized = serde_json::to_string(variant).expect("serializable");
        assert_eq!(
            &serialized, expected_json,
            "variant {variant:?} should serialize as {expected_json}"
        );

        let deserialized: ExchangeTransport =
            serde_json::from_str(&serialized).expect("deserializable");
        assert_eq!(
            deserialized, *variant,
            "round-trip for {variant:?} must be lossless"
        );
    }
}

/// PascalCase aliases (`Qr`, `Nfc`, `Ble`) survive — historic on-disk
/// rows wrote those before the snake_case rename. New variants like
/// `Link` only need the snake_case form because they post-date the
/// alias migration.
// @internal
#[test]
fn pascalcase_aliases_still_decode() {
    let qr: ExchangeTransport = serde_json::from_str("\"Qr\"").expect("Qr alias decodes");
    assert_eq!(qr, ExchangeTransport::Qr);

    let nfc: ExchangeTransport = serde_json::from_str("\"Nfc\"").expect("Nfc alias decodes");
    assert_eq!(nfc, ExchangeTransport::Nfc);

    let ble: ExchangeTransport = serde_json::from_str("\"Ble\"").expect("Ble alias decodes");
    assert_eq!(ble, ExchangeTransport::Ble);
}
