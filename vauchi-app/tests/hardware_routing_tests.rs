// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

#![cfg(feature = "testing")]

//! A hardware shell attaches by reporting what the device can do (ADR-077);
//! from then on Core keeps hardware commands out of the display batches and
//! queues them for the hardware shell. Until one reports, the display shell
//! receives everything, as before.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use vauchi_app::ui::AppEngine;
use vauchi_core::clock::FakeClock;
use vauchi_core::exchange_types::DeviceCapabilities;
use vauchi_core::{Command, CommandTarget, EventJsonError, Vauchi, event_from_json};

fn engine() -> AppEngine {
    let clock = Arc::new(FakeClock::new(
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000),
    ));
    let mut vauchi = Vauchi::in_memory_with_clock(clock).unwrap();
    vauchi.create_identity("Alice").unwrap();
    AppEngine::new(vauchi)
}

fn report_json(device_name: &str) -> String {
    serde_json::json!({"HardwareCapabilitiesReported": {"capabilities": {
        "has_nfc": true, "has_ble": true, "has_camera": true,
        "device_name": device_name
    }}})
    .to_string()
}

fn attach(engine: &mut AppEngine) {
    let report = event_from_json(&report_json("Pixel")).unwrap();
    engine.dispatch(report).unwrap();
}

fn wakeup(engine: &mut AppEngine) -> Vec<Command> {
    engine
        .dispatch(event_from_json(r#""WakeupFired""#).unwrap())
        .unwrap()
}

fn targets(commands: &[Command]) -> Vec<CommandTarget> {
    commands.iter().map(Command::target).collect()
}

fn schedules(commands: &[Command]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, Command::ScheduleWakeup { .. }))
        .count()
}

// @internal — without a hardware shell nothing changes for today's shells.
#[test]
fn without_a_report_the_display_shell_receives_hardware_commands() {
    let mut engine = engine();

    let commands = wakeup(&mut engine);

    assert_eq!(schedules(&commands), 1);
    assert_eq!(engine.drain_hardware_commands(), Vec::new());
}

// @internal — the report reaches Core's capability state (ADR-077 item 7).
#[test]
fn the_report_sets_the_device_capabilities() {
    let mut engine = engine();

    attach(&mut engine);

    assert_eq!(
        engine.device_capabilities(),
        &DeviceCapabilities {
            has_nfc: true,
            has_ble: true,
            has_camera: true,
            device_name: Some("Pixel".into()),
            ..DeviceCapabilities::default()
        }
    );
}

// @internal — after the report, the hardware shell alone receives hardware commands.
#[test]
fn after_the_report_hardware_commands_leave_the_display_batch() {
    let mut engine = engine();
    attach(&mut engine);

    let display = wakeup(&mut engine);
    let hardware = engine.drain_hardware_commands();

    assert!(
        targets(&display)
            .iter()
            .all(|t| *t == CommandTarget::Display),
        "display batch carried {display:?}"
    );
    assert_eq!(schedules(&hardware), 1);
    assert!(
        targets(&hardware)
            .iter()
            .all(|t| *t == CommandTarget::Hardware),
        "hardware queue carried {hardware:?}"
    );
    assert_eq!(engine.drain_hardware_commands(), Vec::new());
}

// @internal — the initial batch is split the same way.
#[test]
fn after_the_report_the_initial_batch_holds_display_commands_only() {
    let mut engine = engine();
    attach(&mut engine);
    wakeup(&mut engine);
    engine.drain_hardware_commands();

    let initial = engine.initial_commands().unwrap();

    assert!(!initial.is_empty());
    assert!(
        targets(&initial)
            .iter()
            .all(|t| *t == CommandTarget::Display),
        "initial batch carried {initial:?}"
    );
}

// @internal — DC-01: the report is bounded at the parse boundary.
#[test]
fn an_oversized_device_name_is_refused_at_the_boundary() {
    assert_eq!(
        event_from_json(&report_json(&"n".repeat(257))),
        Err(EventJsonError::Malformed)
    );
    assert!(event_from_json(&report_json(&"n".repeat(256))).is_ok());
}
