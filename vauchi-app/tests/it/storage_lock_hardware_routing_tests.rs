// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hardware shell can report in while storage is still locked (ADR-077):
//! the lock screen then routes its own hardware command, the platform
//! unlock prompt, to the hardware shell, and keeps the report for the
//! engine that opens afterwards.

use vauchi_app::i18n::Locale;
use vauchi_app::ui::{StorageLockPresentation, StorageLockReason, StorageLockStep};
use vauchi_core::exchange_types::DeviceCapabilities;
use vauchi_core::{Command, Event};

fn locked() -> StorageLockPresentation {
    StorageLockPresentation::new(StorageLockReason::Locked, Locale::ENGLISH)
}

fn capabilities() -> DeviceCapabilities {
    DeviceCapabilities {
        has_biometrics: true,
        ..DeviceCapabilities::default()
    }
}

fn asks_for_prompt(commands: &[Command]) -> bool {
    commands
        .iter()
        .any(|command| matches!(command, Command::RequestBiometricUnlock))
}

// @internal — today's shells still receive the prompt in the display batch.
#[test]
fn without_a_report_the_prompt_stays_in_the_display_batch() {
    let mut lock = locked();

    let batch = lock.initial_commands();

    assert!(asks_for_prompt(&batch));
    assert_eq!(lock.drain_hardware_commands(), Vec::new());
    assert_eq!(lock.hardware_report(), None);
}

// @internal — after a report, the hardware shell receives the prompt.
#[test]
fn after_a_report_the_prompt_goes_to_the_hardware_shell() {
    let mut lock = locked();
    let reported = lock.dispatch(Event::HardwareCapabilitiesReported {
        capabilities: capabilities(),
    });

    let batch = lock.initial_commands();

    assert_eq!(reported, StorageLockStep::Commands(Vec::new()));
    assert!(!asks_for_prompt(&batch), "display batch carried {batch:?}");
    assert_eq!(
        lock.drain_hardware_commands(),
        vec![Command::RequestBiometricUnlock]
    );
}

// @internal — the engine that opens after unlock is handed the report.
#[test]
fn the_lock_keeps_the_report_for_the_engine_that_opens_next() {
    let mut lock = locked();
    lock.dispatch(Event::HardwareCapabilitiesReported {
        capabilities: capabilities(),
    });

    assert_eq!(lock.hardware_report(), Some(&capabilities()));
}
