// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! C ABI batches carry the hardware shell's commands in their own field
//! once it has reported in (ADR-077); before that the field is absent and
//! the batch is exactly what today's shells read. No export is added.

use std::ffi::{CStr, CString};

use vauchi_core::{Command, CommandTarget};

use crate::app::{vauchi_app_create, vauchi_app_create_identity, vauchi_app_destroy};
use crate::app_presentation::{vauchi_app_dispatch, vauchi_app_initial_commands};
use crate::{VauchiApp, vauchi_string_free};

struct App(*mut VauchiApp);

impl App {
    fn with_identity() -> Self {
        // SAFETY: the handle comes from the FFI and is checked before use.
        unsafe {
            let handle = vauchi_app_create();
            assert!(!handle.is_null());
            let name = CString::new("TestUser").unwrap();
            assert_eq!(vauchi_app_create_identity(handle, name.as_ptr()), 0);
            Self(handle)
        }
    }

    fn dispatch(&self, event: serde_json::Value) -> serde_json::Value {
        let event = CString::new(event.to_string()).unwrap();
        // SAFETY: valid handle and NUL-terminated input; the result is read
        // once and freed once.
        unsafe { batch(vauchi_app_dispatch(self.0, event.as_ptr())) }
    }

    fn initial(&self) -> serde_json::Value {
        // SAFETY: valid handle; the result is read once and freed once.
        unsafe { batch(vauchi_app_initial_commands(self.0)) }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        // SAFETY: the handle came from vauchi_app_create and is freed once.
        unsafe { vauchi_app_destroy(self.0) }
    }
}

/// # Safety
/// `ptr` must be a live batch string from the FFI.
unsafe fn batch(ptr: *mut std::os::raw::c_char) -> serde_json::Value {
    assert!(!ptr.is_null(), "the FFI must return a batch");
    // SAFETY: per the contract above; freed right after it is parsed.
    let json = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    unsafe { vauchi_string_free(ptr) };
    serde_json::from_str(&json).unwrap()
}

fn commands(batch: &serde_json::Value, field: &str) -> Vec<Command> {
    serde_json::from_value(batch[field].clone()).unwrap_or_default()
}

fn report() -> serde_json::Value {
    serde_json::json!({"HardwareCapabilitiesReported": {"capabilities": {"has_ble": true}}})
}

// @internal — today's C shells see the batch they always saw.
#[test]
fn before_a_report_a_batch_has_no_hardware_field() {
    let app = App::with_identity();

    let wakeup = app.dispatch(serde_json::json!("WakeupFired"));

    assert!(wakeup.get("hardware").is_none(), "got {wakeup}");
    assert!(
        commands(&wakeup, "commands")
            .iter()
            .any(|command| matches!(command, Command::ScheduleWakeup { .. })),
        "got {wakeup}"
    );
}

// @internal — the hardware shell's commands arrive in their own field.
#[test]
fn after_a_report_hardware_commands_arrive_in_the_hardware_field() {
    let app = App::with_identity();
    app.dispatch(report());

    let wakeup = app.dispatch(serde_json::json!("WakeupFired"));

    let display = commands(&wakeup, "commands");
    let hardware = commands(&wakeup, "hardware");
    assert!(
        display.iter().all(|c| c.target() == CommandTarget::Display),
        "got {wakeup}"
    );
    assert!(
        hardware
            .iter()
            .any(|command| matches!(command, Command::ScheduleWakeup { .. })),
        "got {wakeup}"
    );
    assert!(
        hardware
            .iter()
            .all(|c| c.target() == CommandTarget::Hardware),
        "got {wakeup}"
    );
}

// @internal — a batch with no hardware command keeps the field out.
#[test]
fn after_a_report_a_batch_without_hardware_commands_has_no_field() {
    let app = App::with_identity();
    app.dispatch(report());
    app.dispatch(serde_json::json!("WakeupFired"));

    let initial = app.initial();

    assert!(initial.get("hardware").is_none(), "got {initial}");
    assert!(!commands(&initial, "commands").is_empty());
}
