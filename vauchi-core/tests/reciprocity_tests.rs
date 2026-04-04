// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::contact::Contact;
use vauchi_core::contact_card::ContactCard;
use vauchi_core::crypto::SymmetricKey;
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

fn make_test_contact() -> Contact {
    Contact::from_exchange(
        [1u8; 32],
        ContactCard::new("Alice"),
        SymmetricKey::generate(),
    )
}

#[test]
fn contact_reciprocity_defaults_to_unknown() {
    let contact = make_test_contact();
    assert_eq!(contact.reciprocity(), Reciprocity::Unknown);
}

#[test]
fn contact_set_reciprocity() {
    let mut contact = make_test_contact();
    contact.set_reciprocity(Reciprocity::Confirmed);
    assert_eq!(contact.reciprocity(), Reciprocity::Confirmed);
}

#[test]
fn contact_set_confirmation_channel() {
    let mut contact = make_test_contact();
    contact.set_confirmation_channel(ConfirmationChannel::RelayEscrow);
    assert_eq!(
        contact.confirmation_channel(),
        Some(ConfirmationChannel::RelayEscrow)
    );
}
