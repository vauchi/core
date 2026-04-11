// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! QR exchange sub-flow — screen builders and step logic for
//! Glance (QR-only) and Hover (QR + audio proximity) modes.
//!
//! Extracted from `exchange.rs` to keep the orchestrator lean
//! and make room for Link and BLE sub-flows in later tiers.

use crate::ui::*;
use vauchi_core::exchange::ExchangeSession;

/// Steps specific to the QR exchange sub-flow.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum QrStep {
    ShowQr,
    ScanQr,
    /// Manual code entry — fallback when camera permission is denied.
    ManualEntry,
    Verifying,
}

impl QrStep {
    /// Step number within the overall exchange flow.
    ///
    /// Offsets by `base` so the QR sub-flow slots into the
    /// parent engine's step numbering (e.g., after group
    /// selection).
    pub(super) fn step_number(&self, base: u8) -> u8 {
        base + match self {
            Self::ShowQr => 0,
            Self::ScanQr | Self::ManualEntry => 1,
            Self::Verifying => 2,
        }
    }

    pub(super) const STEP_COUNT: u8 = 3;
}

/// Builds the "Share Your Code" screen.
pub(super) fn build_show_qr_screen(
    session: Option<&ExchangeSession>,
    config_name: &str,
    config_qr_data: &str,
    progress: Progress,
) -> ScreenModel {
    let qr_data = session
        .and_then(|s| s.qr())
        .map(|qr| qr.to_data_string())
        .unwrap_or_else(|| config_qr_data.to_owned());

    ScreenModel {
        screen_id: "exchange_show_qr".into(),
        title: "Share Your Code".into(),
        subtitle: None,
        components: vec![Component::QrCode {
            id: "own_qr".into(),
            data: qr_data,
            mode: QrMode::Display,
            label: Some(config_name.to_owned()),
            a11y: None,
        }],
        actions: vec![ScreenAction {
            id: "continue".into(),
            label: "Scan Their Code".into(),
            style: ActionStyle::Primary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Scan Their Code" screen.
pub(super) fn build_scan_qr_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_scan_qr".into(),
        title: "Scan Their Code".into(),
        subtitle: None,
        components: vec![Component::QrCode {
            id: "scan_qr".into(),
            data: String::new(),
            mode: QrMode::Scan,
            label: None,
            a11y: None,
        }],
        actions: vec![ScreenAction {
            id: "back".into(),
            label: "Back".into(),
            style: ActionStyle::Secondary,
            enabled: true,
        }],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Enter Code Manually" screen — fallback when camera is unavailable.
pub(super) fn build_manual_entry_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_manual_entry".into(),
        title: "Enter Code Manually".into(),
        subtitle: Some("Camera unavailable — ask the other person to read their code".into()),
        components: vec![Component::TextInput {
            id: "manual_code".into(),
            label: "Exchange code".into(),
            value: String::new(),
            placeholder: Some("Paste or type the code".into()),
            max_length: None,
            validation_error: None,
            input_type: InputType::Text,
            a11y: None,
        }],
        actions: vec![
            ScreenAction {
                id: "submit_code".into(),
                label: "Submit".into(),
                style: ActionStyle::Primary,
                enabled: true,
            },
            ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
            },
        ],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Builds the "Verifying" screen.
pub(super) fn build_verifying_screen(progress: Progress) -> ScreenModel {
    ScreenModel {
        screen_id: "exchange_verifying".into(),
        title: "Verifying".into(),
        subtitle: None,
        components: vec![Component::StatusIndicator {
            id: "verifying_status".into(),
            icon: None,
            title: "Verifying...".into(),
            detail: None,
            status: Status::InProgress,
            a11y: None,
        }],
        actions: vec![],
        progress: Some(progress),
        ..Default::default()
    }
}

/// Handle a user action while in a QR sub-flow step.
///
/// Returns `Some(result)` if the action was handled, `None` if
/// it should fall through to the parent engine.
pub(super) fn handle_qr_action(
    step: &QrStep,
    action: &UserAction,
    session_active: bool,
) -> Option<QrActionOutcome> {
    match (step, action) {
        (QrStep::ShowQr, UserAction::ActionPressed { action_id }) if action_id == "continue" => {
            Some(QrActionOutcome::AdvanceToScan { session_active })
        }
        (QrStep::ScanQr, UserAction::ActionPressed { action_id }) if action_id == "back" => {
            Some(QrActionOutcome::BackToShowQr)
        }
        (
            QrStep::ScanQr,
            UserAction::TextChanged {
                component_id,
                value,
            },
        ) if component_id == "scanned_data" => Some(QrActionOutcome::QrScanned {
            data: value.clone(),
        }),
        // Manual entry: submit the typed code
        (QrStep::ManualEntry, UserAction::ActionPressed { action_id })
            if action_id == "submit_code" =>
        {
            None
        } // Handled by parent via TextChanged
        (QrStep::ManualEntry, UserAction::ActionPressed { action_id }) if action_id == "back" => {
            Some(QrActionOutcome::BackToShowQr)
        }
        (
            QrStep::ManualEntry,
            UserAction::TextChanged {
                component_id,
                value,
            },
        ) if component_id == "manual_code" => Some(QrActionOutcome::ManualCodeEntered {
            data: value.clone(),
        }),
        _ => None,
    }
}

/// Outcome of a QR sub-flow action, interpreted by the parent engine.
pub(super) enum QrActionOutcome {
    /// User pressed "Scan Their Code" — advance to ScanQr step.
    AdvanceToScan { session_active: bool },
    /// User pressed "Back" on scan/manual screen — return to ShowQr.
    BackToShowQr,
    /// User scanned a QR code — store data and move to Verifying.
    QrScanned { data: String },
    /// User submitted a code via manual entry (camera permission denied fallback).
    ManualCodeEntered { data: String },
}
