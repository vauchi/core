// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! UniFFI batches carry the hardware shell's commands in their own field once
//! it has reported in (ADR-077); before that the field is absent and the
//! batch is exactly what today's shells read.

use vauchi_core::{Command, CommandTarget};
use vauchi_platform::PlatformAppEngine;
use vauchi_platform::PlatformAppEngineTestHelpers;

fn create_engine() -> (std::sync::Arc<PlatformAppEngine>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let key = vauchi_core::crypto::SymmetricKey::generate();
    let engine = PlatformAppEngine::open_under_shell_key(
        dir.path().to_string_lossy().to_string(),
        "https://relay.test".into(),
        key.as_bytes().to_vec(),
    )
    .expect("create engine");
    (engine, dir)
}

fn dispatch(engine: &PlatformAppEngine, event: serde_json::Value) -> serde_json::Value {
    serde_json::from_str(&engine.dispatch_json(event.to_string()).unwrap()).unwrap()
}

fn commands(batch: &serde_json::Value, field: &str) -> Vec<Command> {
    serde_json::from_value(batch[field].clone()).unwrap_or_default()
}

fn has_schedule(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::ScheduleWakeup { .. }))
}

fn report() -> serde_json::Value {
    serde_json::json!({"HardwareCapabilitiesReported": {"capabilities": {"has_ble": true}}})
}

// @internal — today's mobile shells see the batch they always saw.
#[test]
fn before_a_report_a_batch_has_no_hardware_field() {
    let (engine, _dir) = create_engine();

    let wakeup = dispatch(&engine, serde_json::json!("WakeupFired"));

    assert!(wakeup.get("hardware").is_none(), "got {wakeup}");
    assert!(has_schedule(&commands(&wakeup, "commands")), "got {wakeup}");
}

// @internal — the hardware shell's commands arrive in their own field.
#[test]
fn after_a_report_hardware_commands_arrive_in_the_hardware_field() {
    let (engine, _dir) = create_engine();
    dispatch(&engine, report());

    let wakeup = dispatch(&engine, serde_json::json!("WakeupFired"));

    let display = commands(&wakeup, "commands");
    let hardware = commands(&wakeup, "hardware");
    assert!(
        display.iter().all(|c| c.target() == CommandTarget::Display),
        "got {wakeup}"
    );
    assert!(has_schedule(&hardware), "got {wakeup}");
    assert!(
        hardware
            .iter()
            .all(|c| c.target() == CommandTarget::Hardware),
        "got {wakeup}"
    );
}

// @internal — the legacy wakeup envelope splits the same way.
#[test]
fn after_a_report_the_legacy_wakeup_envelope_has_a_hardware_field() {
    let (engine, _dir) = create_engine();
    dispatch(&engine, report());

    let envelope: serde_json::Value = serde_json::from_str(&engine.on_wakeup().unwrap()).unwrap();

    let display = commands(&envelope, "commands");
    assert!(
        display.iter().all(|c| c.target() == CommandTarget::Display),
        "got {envelope}"
    );
    assert!(
        has_schedule(&commands(&envelope, "hardware")),
        "got {envelope}"
    );
}
