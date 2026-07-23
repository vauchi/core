// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the BLE exchange flow via the ADR-031 command/event protocol.
//!
//! These tests verify that `ExchangeSession` drives `BleHandshakeSession`
//! through the full 4-phase handshake using only `Command`s and
//! `Event`s -- no hardware traits, no mock transports.

use vauchi_core::ContactCard;
use vauchi_core::exchange::{
    CHAR_DATA_NOTIFY, CHAR_DATA_WRITE, CHAR_HANDSHAKE_NOTIFY, CHAR_HANDSHAKE_WRITE,
    ExchangeSession, ExchangeState, ManualConfirmationVerifier, VAUCHI_BLE_SERVICE_UUID,
};
use vauchi_core::identity::Identity;
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::{Command, Event};

/// Helper: create a BLE exchange session with a fresh identity.
fn ble_session(name: &str) -> ExchangeSession {
    let identity = Identity::create(name, 0);
    let card = ContactCard::new(name);
    let proximity = ManualConfirmationVerifier::new();
    ExchangeSession::new_ble(
        identity,
        card,
        proximity,
        vauchi_core::clock::SystemClock::shared(),
    )
}

// -- Initial commands ------------------------------------------------

// @internal
#[test]
fn ble_session_emits_scan_and_advertise_on_start() {
    let mut session = ble_session("Alice");
    session.emit_initial_commands();
    let cmds = session.drain_commands();

    assert_eq!(cmds.len(), 2, "expected scan + advertise commands");

    let has_scan = cmds.iter().any(|c| {
        matches!(c, Command::BleStartScanning { service_uuid }
            if service_uuid == VAUCHI_BLE_SERVICE_UUID)
    });
    let has_advertise = cmds.iter().any(|c| {
        matches!(c, Command::BleStartAdvertising { service_uuid, .. }
            if service_uuid == VAUCHI_BLE_SERVICE_UUID)
    });

    assert!(has_scan, "missing BleStartScanning command");
    assert!(has_advertise, "missing BleStartAdvertising command");
}

// -- Discovery -> Connect ---------------------------------------------

// @internal
#[test]
fn ble_device_discovered_emits_connect_command() {
    let mut session = ble_session("Alice");

    session
        .apply_hardware_event(Event::BleDeviceDiscovered {
            id: "peer-1".into(),
            rssi: -42,
            adv_data: vec![],
        })
        .unwrap();

    let cmds = session.drain_commands();
    assert_eq!(cmds.len(), 2, "expected BleStopScanning + BleConnect");
    assert!(
        matches!(&cmds[0], Command::BleStopScanning),
        "expected BleStopScanning first, got {:?}",
        cmds[0]
    );
    assert!(
        matches!(&cmds[1], Command::BleConnect { device_id } if device_id == "peer-1"),
        "expected BleConnect second, got {:?}",
        cmds[1]
    );
}

// -- BleConnected (initiator) -> KeyOffer write -----------------------

// @internal
#[test]
fn ble_connected_after_discovery_emits_key_offer_write() {
    let mut session = ble_session("Alice");

    // Discovery marks us as initiator
    session
        .apply_hardware_event(Event::BleDeviceDiscovered {
            id: "peer-1".into(),
            rssi: -42,
            adv_data: vec![],
        })
        .unwrap();
    let _ = session.drain_commands(); // drain BleConnect

    session
        .apply_hardware_event(Event::BleConnected {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
        })
        .unwrap();

    let cmds = session.drain_commands();
    assert!(
        !cmds.is_empty(),
        "BleConnected (initiator) should emit commands"
    );

    let write_cmd = cmds.iter().find(|c| {
        matches!(c, Command::BleWriteCharacteristic { uuid, data, .. }
            if uuid == CHAR_HANDSHAKE_WRITE && !data.is_empty())
    });
    assert!(
        write_cmd.is_some(),
        "expected BleWriteCharacteristic to CHAR_HANDSHAKE_WRITE with KeyOffer data, got {:?}",
        cmds
    );
}

// -- Full 4-phase initiator flow -------------------------------------

// @internal
#[test]
fn ble_full_initiator_flow_via_command_event() {
    let mut initiator = ble_session("Alice");
    let mut responder_hs = ble_session("Bob");

    // --- Step 1: Discovery + connect ---
    initiator
        .apply_hardware_event(Event::BleDeviceDiscovered {
            id: "bob-device".into(),
            rssi: -30,
            adv_data: vec![],
        })
        .unwrap();
    let _ = initiator.drain_commands(); // BleConnect

    initiator
        .apply_hardware_event(Event::BleConnected {
            device_id: "bob-device".into(),
            direction: BleLinkDirection::Outbound,
        })
        .unwrap();

    let cmds = initiator.drain_commands();
    // Extract KeyOffer from the write command
    let key_offer = cmds
        .iter()
        .find_map(|c| match c {
            Command::BleWriteCharacteristic { uuid, data, .. } if uuid == CHAR_HANDSHAKE_WRITE => {
                Some(data.clone())
            }
            _ => None,
        })
        .expect("initiator should emit KeyOffer write");

    assert_eq!(key_offer.len(), 137, "v4 KeyOffer should be 137 bytes");

    // --- Step 2: Responder processes KeyOffer (simulated) ---
    // Use Bob's BleHandshakeSession directly to produce the response
    let bob_hs = responder_hs
        .ble_handshake_mut()
        .expect("Bob should have a BLE handshake session");
    let (key_ack, bob_encrypted_card) = bob_hs
        .process_key_offer(
            &key_offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .unwrap();

    assert_eq!(key_ack.len(), 153, "v3 KeyAck should be 153 bytes");

    // Feed KeyAck and encrypted card back to initiator as hardware events
    initiator
        .apply_hardware_event(Event::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_HANDSHAKE_NOTIFY.into(),
            data: key_ack,
        })
        .unwrap();

    initiator
        .apply_hardware_event(Event::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: bob_encrypted_card.clone(),
        })
        .unwrap();

    let cmds = initiator.drain_commands();
    let commitment_write = cmds.iter().find(|c| {
        matches!(c, Command::BleWriteCharacteristic { uuid, .. }
            if uuid == CHAR_HANDSHAKE_WRITE)
    });
    let card_write = cmds.iter().find(|c| {
        matches!(c, Command::BleWriteCharacteristic { uuid, .. }
            if uuid == CHAR_DATA_WRITE)
    });
    assert!(
        commitment_write.is_some(),
        "Phase 3: expected commitment write to CHAR_HANDSHAKE_WRITE, got {:?}",
        cmds
    );
    assert!(
        card_write.is_some(),
        "Phase 3: expected encrypted card write to CHAR_DATA_WRITE, got {:?}",
        cmds
    );

    // Extract Phase 3 data for responder processing
    let our_commitment = match commitment_write.unwrap() {
        Command::BleWriteCharacteristic { data, .. } => data.clone(),
        _ => unreachable!(),
    };
    let our_encrypted_card = match card_write.unwrap() {
        Command::BleWriteCharacteristic { data, .. } => data.clone(),
        _ => unreachable!(),
    };

    // --- Step 4: Responder processes Phase 3 (simulated) ---
    let reveal = bob_hs
        .process_committed_payload(&our_commitment, &our_encrypted_card)
        .unwrap();

    // Feed reveal back to initiator
    initiator
        .apply_hardware_event(Event::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_HANDSHAKE_NOTIFY.into(),
            data: reveal,
        })
        .unwrap();

    assert!(
        matches!(initiator.state(), ExchangeState::Complete { .. }),
        "initiator should be in Complete state, got {:?}",
        initiator.state()
    );
}

// -- BLE disconnect during handshake -> fail --------------------------

// @internal
#[test]
fn ble_disconnect_during_connection_fails_session() {
    let mut session = ble_session("Alice");

    session
        .apply_hardware_event(Event::BleDisconnected {
            device_id: "peer-1".into(),
            direction: vauchi_core::BleLinkDirection::Outbound,
            reason: "remote closed".into(),
        })
        .unwrap();

    assert!(
        matches!(session.state(), ExchangeState::Failed { .. }),
        "BLE disconnect in AwaitingBleConnection should fail the session"
    );
}

// -- BLE hardware error -> fail ---------------------------------------

// @internal
#[test]
fn ble_hardware_error_fails_session() {
    let mut session = ble_session("Alice");

    session
        .apply_hardware_event(Event::HardwareError {
            transport: "BLE".into(),
            error: "adapter disabled".into(),
        })
        .unwrap();

    assert!(
        matches!(session.state(), ExchangeState::Failed { .. }),
        "hardware error should fail the session"
    );
}

// -- Out-of-order BLE data buffering ---------------------------------

// @internal
#[test]
fn ble_card_before_key_ack_is_buffered_and_processed() {
    let mut initiator = ble_session("Alice");
    let mut responder = ble_session("Bob");

    // Discovery + connect + KeyOffer
    initiator
        .apply_hardware_event(Event::BleDeviceDiscovered {
            id: "bob".into(),
            rssi: -30,
            adv_data: vec![],
        })
        .unwrap();
    let _ = initiator.drain_commands();
    initiator
        .apply_hardware_event(Event::BleConnected {
            device_id: "bob".into(),
            direction: BleLinkDirection::Outbound,
        })
        .unwrap();

    let cmds = initiator.drain_commands();
    let key_offer = cmds
        .iter()
        .find_map(|c| match c {
            Command::BleWriteCharacteristic { uuid, data, .. } if uuid == CHAR_HANDSHAKE_WRITE => {
                Some(data.clone())
            }
            _ => None,
        })
        .unwrap();

    let bob_hs = responder.ble_handshake_mut().unwrap();
    let (key_ack, encrypted_card) = bob_hs
        .process_key_offer(
            &key_offer,
            vauchi_core::clock::SystemClock::shared().unix_seconds(),
        )
        .unwrap();

    // Feed card data BEFORE key_ack (reversed order)
    initiator
        .apply_hardware_event(Event::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_DATA_NOTIFY.into(),
            data: encrypted_card,
        })
        .unwrap();

    // No commands yet -- waiting for key_ack
    let cmds = initiator.drain_commands();
    assert!(
        cmds.is_empty(),
        "should not emit commands until both key_ack and card data arrive"
    );

    initiator
        .apply_hardware_event(Event::BleCharacteristicNotified {
            device_id: "peer-1".into(),
            direction: BleLinkDirection::Outbound,
            uuid: CHAR_HANDSHAKE_NOTIFY.into(),
            data: key_ack,
        })
        .unwrap();

    let cmds = initiator.drain_commands();
    assert!(
        !cmds.is_empty(),
        "after both key_ack and card data arrive, should emit Phase 3 commands"
    );
}
