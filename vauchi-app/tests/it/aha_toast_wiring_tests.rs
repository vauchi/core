// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A milestone Core's ledger fires reaches every shell as a prepared
//! `Command::ShowToast`, after the exchange ceremony when both arrive in
//! one batch (vauchi/private#276 D2.5, #848). Shells never ask Core
//! whether a milestone fired (ADR-066).

use vauchi_app::i18n::{Locale, get_string_with_args};
use vauchi_app::ui::AppEngine;
use vauchi_core::api::Vauchi;
use vauchi_core::platform::BleLinkDirection;
use vauchi_core::{Command, Contact, ContactCard, Event, SymmetricKey};

fn engine_named(name: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    AppEngine::new(vauchi)
}

fn token_of(engine: &AppEngine) -> Vec<u8> {
    engine
        .vauchi()
        .identity()
        .expect("identity")
        .signing_public_key()
        .to_vec()
}

fn first_contact_toast(name: &str) -> String {
    let title = vauchi_app::i18n::get_string(Locale::ENGLISH, "aha.first_contact_added.title");
    let message = get_string_with_args(
        Locale::ENGLISH,
        "aha.first_contact_added.message_with_name",
        &[("name", name)],
    );
    format!("{title} {message}")
}

fn toast_messages(commands: &[Command]) -> Vec<String> {
    commands
        .iter()
        .filter_map(|cmd| match cmd {
            Command::ShowToast { toast } => Some(toast.message.clone()),
            _ => None,
        })
        .collect()
}

fn position<F: Fn(&Command) -> bool>(commands: &[Command], pred: F) -> Option<usize> {
    commands.iter().position(pred)
}

/// Routes BLE writes between two engines until neither has more to say,
/// collecting every command each side drained along the way.
fn run_ble_exchange(alice: &mut AppEngine, bob: &mut AppEngine) -> (Vec<Command>, Vec<Command>) {
    let alice_token = token_of(alice);
    let bob_token = token_of(bob);
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

    let mut alice_seen = Vec::new();
    let mut bob_seen = Vec::new();
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
            alice_seen.push(cmd);
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
            bob_seen.push(cmd);
        }
        if routed == 0 {
            break;
        }
    }
    assert!(
        !alice.vauchi().list_contacts().unwrap().is_empty(),
        "exchange must complete for the assertions to mean anything"
    );
    (alice_seen, bob_seen)
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_first_exchange_toasts_the_milestone_once_per_side_after_the_ceremony() {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");

    let (alice_seen, bob_seen) = run_ble_exchange(&mut alice, &mut bob);

    for (side, seen, peer) in [("Alice", &alice_seen, "Bob"), ("Bob", &bob_seen, "Alice")] {
        assert_eq!(
            toast_messages(seen),
            vec![first_contact_toast(peer)],
            "{side} sees exactly one milestone toast, naming the contact the exchange added"
        );
        let celebrate = position(seen, |c| matches!(c, Command::Celebrate { .. }))
            .unwrap_or_else(|| panic!("{side} celebrates"));
        let toast = position(seen, |c| matches!(c, Command::ShowToast { .. }))
            .unwrap_or_else(|| panic!("{side} is toasted"));
        assert!(
            celebrate < toast,
            "{side}: the ceremony wins; the toast follows it (celebrate at {celebrate}, toast at {toast})"
        );
    }
}

// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_later_contact_toasts_nothing() {
    let mut alice = engine_named("Alice");
    let mut bob = engine_named("Bob");
    run_ble_exchange(&mut alice, &mut bob);
    alice
        .vauchi()
        .add_contact(Contact::from_exchange(
            [9u8; 32],
            ContactCard::new("Erin"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();

    let drained = alice.drain_pending_commands();

    assert_eq!(
        toast_messages(&drained),
        Vec::<String>::new(),
        "the ledger fires each milestone once, so a later contact brings no toast"
    );
}

/// A milestone with no ceremony around it (a contact added outside an
/// exchange, a sync milestone) is toasted on its own.
// @scenario: onboarding :: Exchange success celebration
// @internal
#[test]
fn a_milestone_without_a_ceremony_is_toasted_on_its_own() {
    let mut engine = engine_named("Owner");
    engine
        .vauchi()
        .add_contact(Contact::from_exchange(
            [7u8; 32],
            ContactCard::new("Dana"),
            SymmetricKey::generate(),
            0,
        ))
        .unwrap();

    let drained = engine.drain_pending_commands();

    assert_eq!(toast_messages(&drained), vec![first_contact_toast("Dana")]);
    assert!(
        position(&drained, |c| matches!(c, Command::Celebrate { .. })).is_none(),
        "no exchange, no ceremony"
    );
}
