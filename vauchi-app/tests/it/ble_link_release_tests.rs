// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A completed BLE exchange releases its link (vauchi/private#673). After a
//! Glance the iPhone dialled, the iPhone sat on Success with the link up and
//! the Pixel's next Glance found that stale central already connected, so it
//! stuck on "Exchanging". The initiator, which completes last, now queues a
//! disconnect after its final write.

use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::{Command, Event};

fn engine_named(name: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    AppEngine::new(vauchi)
}

fn token_of(engine: &AppEngine) -> Vec<u8> {
    engine.ble_attempt_token().to_vec()
}

/// Runs a full two-party exchange, Alice dialling Bob, and returns every
/// command each side emitted, in order.
fn exchange() -> (AppEngine, AppEngine, Vec<Command>, Vec<Command>) {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");
    let (alice_token, bob_token) = (token_of(&alice), token_of(&bob));
    alice.start_ble_handshake_on_discovery(&bob_token);
    bob.start_ble_handshake_on_discovery(&alice_token);
    let ea = alice.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "bob".into(),
        direction: BleLinkDirection::Outbound,
    });
    alice.apply_ble_machine_event(ea);
    let eb = bob.forward_ble_hardware_event(&Event::BleConnected {
        device_id: "alice".into(),
        direction: BleLinkDirection::Inbound,
    });
    bob.apply_ble_machine_event(eb);

    let (mut from_alice, mut from_bob) = (Vec::new(), Vec::new());
    for _ in 0..50 {
        let mut routed = 0;
        for cmd in alice.drain_pending_commands() {
            if let Command::BleWriteCharacteristic { uuid, data, .. } = &cmd {
                routed += 1;
                let ev = bob.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id: String::new(),
                    direction: BleLinkDirection::Inbound,
                    uuid: uuid.clone(),
                    data: data.clone(),
                });
                bob.apply_ble_machine_event(ev);
            }
            from_alice.push(cmd);
        }
        for cmd in bob.drain_pending_commands() {
            if let Command::BleWriteCharacteristic { uuid, data, .. } = &cmd {
                routed += 1;
                let ev = alice.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id: String::new(),
                    direction: BleLinkDirection::Outbound,
                    uuid: uuid.clone(),
                    data: data.clone(),
                });
                alice.apply_ble_machine_event(ev);
            }
            from_bob.push(cmd);
        }
        if routed == 0 {
            break;
        }
    }
    (alice, bob, from_alice, from_bob)
}

fn disconnects(commands: &[Command]) -> Vec<usize> {
    commands
        .iter()
        .enumerate()
        .filter(|(_, c)| matches!(c, Command::BleDisconnect { .. }))
        .map(|(i, _)| i)
        .collect()
}

fn last_write(commands: &[Command]) -> Option<usize> {
    commands
        .iter()
        .rposition(|c| matches!(c, Command::BleWriteCharacteristic { .. }))
}

// @internal
#[test]
fn only_the_initiator_releases_the_link_after_its_last_write() {
    let (alice, bob, from_alice, from_bob) = exchange();
    assert!(!alice.vauchi().list_contacts().unwrap().is_empty());
    assert!(!bob.vauchi().list_contacts().unwrap().is_empty());

    let (releasing, other) = match (disconnects(&from_alice).len(), disconnects(&from_bob).len()) {
        (1, 0) => (&from_alice, &from_bob),
        (0, 1) => (&from_bob, &from_alice),
        counts => panic!("exactly one side releases the link, got {counts:?}"),
    };
    assert!(disconnects(other).is_empty());
    assert!(
        disconnects(releasing)[0] > last_write(releasing).expect("the releasing side wrote"),
        "the release follows the side's last write (its reciprocity ack): {releasing:?}"
    );
}

// @internal
#[test]
fn the_peer_keeps_its_contact_when_the_released_link_drops() {
    let (mut alice, mut bob, from_alice, _) = exchange();
    // The side that did not release sees the link drop; its direction is the
    // one it had on the link (Bob was dialled into, Alice dialled out).
    let (peer, direction) = if disconnects(&from_alice).is_empty() {
        (&mut alice, BleLinkDirection::Outbound)
    } else {
        (&mut bob, BleLinkDirection::Inbound)
    };

    let ev = peer.forward_ble_hardware_event(&Event::BleDisconnected {
        device_id: String::new(),
        direction,
        reason: "link released".into(),
    });
    peer.apply_ble_machine_event(ev);

    assert_eq!(peer.vauchi().list_contacts().unwrap().len(), 1);
}
