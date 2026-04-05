// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE exchange sub-flow — screen builders and step logic for
//! Magic (BLE + audio), Bump (BLE + impact), and Shake (BLE +
//! accelerometer correlation) modes.
//!
//! Follows the `exchange_qr.rs` / `exchange_link.rs` pattern:
//! steps, screen builders, action/hardware-event handlers that
//! return outcomes for the parent engine to act on.

use crate::ui::*;
use vauchi_core::exchange::command::ExchangeCommand;
use vauchi_core::exchange::mode::ExchangeMode;

// ── Step enum ──────────────────────────────────────────────────────────────

/// Steps specific to the BLE exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // Variants scaffolded for Phase 1–3
pub(super) enum BleStep {
    /// Scanning for nearby BLE peers.
    Discovering,
    /// Running BLE handshake protocol (key exchange).
    Handshaking,
    /// Exchanging contact card data over BLE GATT.
    Exchanging,
    /// Running proximity verification (audio/impact/accel).
    Verifying,
    /// BLE exchange complete, results ready.
    Complete,
}

impl BleStep {
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::Discovering => 0,
            Self::Handshaking => 1,
            Self::Exchanging => 1,
            Self::Verifying => 2,
            Self::Complete => 2,
        }
    }

    /// Matches QrStep/LinkStep for consistent progress bar.
    pub(super) const STEP_COUNT: u8 = 3;
}

// ── Action/hardware outcomes ───────────────────────────────────────────────

/// Result of handling a user action in the BLE sub-flow.
#[allow(dead_code)] // Ignored variant used in Phase 1
pub(super) enum BleActionOutcome {
    /// No state change — action not handled by BLE flow.
    Ignored,
    /// User accepted relay fallback after BLE timeout.
    FallbackToRelay,
    /// User cancelled.
    Cancel,
}

/// Result of handling a hardware event in the BLE sub-flow.
#[allow(dead_code)] // Variants scaffolded for Phase 1–3
pub(super) enum BleHardwareOutcome {
    /// Step advanced — parent should update screen.
    StepAdvanced,
    /// BLE exchange completed — card bytes available.
    Complete {
        card_bytes: Vec<u8>,
        commands: Vec<ExchangeCommand>,
    },
    /// BLE failed — offer relay fallback.
    FailedWithFallback { reason: String },
    /// Event consumed but no step change.
    Consumed,
    /// Event not handled by BLE flow.
    Ignored,
}

// ── Screen builders ────────────────────────────────────────────────────────

pub(super) fn build_discovering_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let (title, subtitle) = match mode {
        ExchangeMode::Magic => (
            "Searching nearby...",
            "Hold your phone near the other device",
        ),
        ExchangeMode::Bump => ("Ready to bump", "Bump your phones together to exchange"),
        ExchangeMode::Shake => ("Ready to shake", "Shake both phones together to exchange"),
        _ => ("Searching...", "Looking for nearby devices"),
    };

    ScreenModel {
        screen_id: "exchange_ble_discovering".into(),
        title: title.into(),
        subtitle: Some(subtitle.into()),
        components: vec![Component::Text {
            id: "ble_status".into(),
            content: "Scanning for nearby devices...".into(),
            style: TextStyle::Body,
        }],
        actions: vec![ScreenAction {
            id: "cancel".into(),
            label: "Cancel".into(),
            style: ActionStyle::Secondary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

pub(super) fn build_exchanging_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let title = match mode {
        ExchangeMode::Magic => "Exchanging cards",
        ExchangeMode::Bump => "Exchanging cards",
        ExchangeMode::Shake => "Exchanging cards",
        _ => "Exchanging...",
    };

    ScreenModel {
        screen_id: "exchange_ble_exchanging".into(),
        title: title.into(),
        subtitle: Some("Transferring contact information securely".into()),
        components: vec![Component::Text {
            id: "ble_exchange_status".into(),
            content: "Encrypted exchange in progress...".into(),
            style: TextStyle::Body,
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

pub(super) fn build_verifying_screen(mode: ExchangeMode, progress: Progress) -> ScreenModel {
    let subtitle = match mode {
        ExchangeMode::Magic => "Confirming proximity via audio...",
        ExchangeMode::Bump => "Confirming proximity via impact...",
        ExchangeMode::Shake => "Confirming proximity via motion...",
        _ => "Verifying proximity...",
    };

    ScreenModel {
        screen_id: "exchange_ble_verifying".into(),
        title: "Verifying".into(),
        subtitle: Some(subtitle.into()),
        components: vec![],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

// ── Action handler ─────────────────────────────────────────────────────────

pub(super) fn handle_ble_action(step: &BleStep, action: &UserAction) -> Option<BleActionOutcome> {
    match (step, action) {
        (_, UserAction::ActionPressed { action_id }) if action_id == "cancel" => {
            Some(BleActionOutcome::Cancel)
        }
        (_, UserAction::ActionPressed { action_id }) if action_id == "fallback_relay" => {
            Some(BleActionOutcome::FallbackToRelay)
        }
        _ => None,
    }
}
