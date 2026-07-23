// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! M2 S6 (design D2.6, `2026-07-03-exchange-ceremony-unshipped` goal 4 +
//! `2026-06-04-exchange-terminal-screens` "remaining gap"): a completed BLE
//! exchange ends on the rich shared success summary — who you met and what
//! they shared — not the minimal "Exchange Complete" indicator. Unblocked by
//! the machine-path persistence (reciprocity P1): the AppEngine persists the
//! contact, so it can build the summary and hand it to the hollow chrome.

use vauchi_app::ui::{AppEngine, AppScreen, Component, WorkflowEngine};
use vauchi_core::Event;
use vauchi_core::api::Vauchi;
use vauchi_core::contact_card::{ContactField, FieldType};
use vauchi_core::exchange::mode::ExchangeMode;
use vauchi_core::platform::BleLinkDirection;

fn engine_named(name: &str, email: &str) -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity(name).expect("identity");
    let mut card = vauchi
        .own_card()
        .expect("own_card")
        .expect("card exists after identity");
    card.add_field(ContactField::new(FieldType::Email, "Email", email, 0))
        .expect("add email");
    vauchi.update_own_card(&card).expect("update card");
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

fn screen_text(engine: &AppEngine) -> String {
    let screen = engine.current_screen();
    let mut out = vec![screen.title.clone()];
    for c in &screen.components {
        match c {
            Component::Text { content, .. } => out.push(content.clone()),
            Component::StatusIndicator { title, detail, .. } => {
                out.push(title.clone());
                if let Some(d) = detail {
                    out.push(d.clone());
                }
            }
            Component::List { items, .. } => {
                for i in items {
                    out.push(format!(
                        "{} {}",
                        i.name,
                        i.subtitle.clone().unwrap_or_default()
                    ));
                }
            }
            _ => {}
        }
    }
    out.join(" | ")
}

// A completed BLE exchange renders the rich shared success summary on the
// BLE chrome — the peer's name (and their shared fields) appear.
// @scenario: exchange :: BLE ends on the rich success summary
// @internal
#[test]
fn ble_completion_renders_rich_success_summary() {
    let mut alice = engine_named("Alice", "alice@a.test");
    let mut bob = engine_named("Bob", "bob@b.test");
    let alice_token = token_of(&alice);
    let bob_token = token_of(&bob);

    // Both sides sit on the BLE exchange chrome (as on-device).
    alice.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Bump,
    });
    bob.navigate_to(AppScreen::BleExchange {
        mode: ExchangeMode::Bump,
    });
    let _ = alice.drain_pending_commands();
    let _ = bob.drain_pending_commands();

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

    for _ in 0..50 {
        let mut routed = 0;
        for cmd in alice.drain_pending_commands() {
            if let vauchi_core::Command::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            } = cmd
            {
                routed += 1;
                let ev = bob.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id,
                    uuid,
                    data,
                });
                bob.apply_ble_machine_event(ev);
            }
        }
        for cmd in bob.drain_pending_commands() {
            if let vauchi_core::Command::BleWriteCharacteristic {
                device_id,
                uuid,
                data,
            } = cmd
            {
                routed += 1;
                let ev = alice.forward_ble_hardware_event(&Event::BleCharacteristicNotified {
                    device_id,
                    uuid,
                    data,
                });
                alice.apply_ble_machine_event(ev);
            }
        }
        if routed == 0 {
            break;
        }
    }
    assert!(
        !alice.vauchi().list_contacts().unwrap().is_empty(),
        "exchange must complete"
    );

    let alice_screen = alice.current_screen();
    assert_eq!(
        alice_screen.screen_id, "exchange_success",
        "BLE ends on the shared rich success screen"
    );
    let text = screen_text(&alice);
    assert!(
        text.contains("Bob"),
        "the summary names the new contact; screen text: {text}"
    );
}
