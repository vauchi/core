// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::reciprocity::{ConfirmationChannel, Reciprocity};

#[test]
fn reciprocity_serde_roundtrip() {
    for variant in [
        Reciprocity::Confirmed,
        Reciprocity::Pending,
        Reciprocity::Unreciprocated,
        Reciprocity::Unknown,
    ] {
        let json = serde_json::to_string(&variant).expect("serialize");
        let back: Reciprocity = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, back, "roundtrip failed for {json}");
    }
}

#[test]
fn confirmation_channel_serde_roundtrip() {
    for variant in [
        ConfirmationChannel::Audio,
        ConfirmationChannel::Ble,
        ConfirmationChannel::RelayEscrow,
        ConfirmationChannel::RelaySync,
    ] {
        let json = serde_json::to_string(&variant).expect("serialize");
        let back: ConfirmationChannel = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(variant, back, "roundtrip failed for {json}");
    }
}

#[test]
fn reciprocity_snake_case_serialization() {
    assert_eq!(
        serde_json::to_string(&Reciprocity::Unreciprocated).unwrap(),
        "\"unreciprocated\""
    );
    assert_eq!(
        serde_json::to_string(&ConfirmationChannel::RelayEscrow).unwrap(),
        "\"relay_escrow\""
    );
}
