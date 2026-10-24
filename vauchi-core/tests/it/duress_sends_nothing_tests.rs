// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A duress unlock sends nothing: any alert it sent could reveal that the
//! duress PIN was used (ADR-032 amendment 2026-10-11, vauchi/private#636).

use vauchi_core::api::AuthMode;
use vauchi_core::sync::safety_alert::SafetyAlertPayload;
use vauchi_core::{Contact, ContactCard};

use crate::common::helpers::create_vauchi_with_identity;

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn a_duress_unlock_queues_nothing_for_a_ratcheted_contact() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").unwrap();
    wb.setup_duress_password("654321").unwrap();
    let friend = Contact::from_exchange(
        [7; 32],
        ContactCard::new("Trusted Friend"),
        vauchi_core::crypto::SymmetricKey::generate(),
        0,
    );
    let friend_id = friend.id().to_string();
    wb.add_contact(friend).unwrap();
    let dh = vauchi_core::exchange::X3DHKeyPair::generate();
    wb.create_ratchet_as_initiator(
        &friend_id,
        &vauchi_core::crypto::SymmetricKey::generate(),
        *dh.public_key(),
    )
    .unwrap();

    let mode = wb.authenticate("654321").unwrap();

    assert_eq!(mode, AuthMode::Duress);
    assert_eq!(
        wb.storage().pending().count_all_pending_updates().unwrap(),
        0,
        "a duress unlock must leave nothing queued for anyone"
    );
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn an_alert_claiming_to_be_a_duress_alert_does_not_decode() {
    let mut wire = vec![0u8; 32 + 64];
    wire.extend_from_slice(br#"{"kind":"Duress","message":"help","timestamp":1,"location":null}"#);

    let decoded = SafetyAlertPayload::decode(&wire);

    assert!(
        decoded.is_err(),
        "only emergency alerts exist; a duress kind is unknown"
    );
}

// @internal
#[test]
fn an_emergency_alert_still_decodes() {
    let mut wire = vec![0u8; 32 + 64];
    wire.extend_from_slice(
        br#"{"kind":"Emergency","message":"help","timestamp":1,"location":null}"#,
    );

    assert!(SafetyAlertPayload::decode(&wire).is_ok());
}

// @scenario: duress_mode :: A duress unlock sends nothing
#[test]
fn a_duress_pin_alone_counts_as_duress_set_up() {
    let mut wb = create_vauchi_with_identity("Alice");
    wb.setup_app_password("my-pin-1234").unwrap();
    wb.setup_duress_password("654321").unwrap();

    let status = wb.get_emergency_wipe_status().unwrap();

    assert!(
        status.duress_configured,
        "duress protection is the PIN; there are no alert settings to require"
    );
}
