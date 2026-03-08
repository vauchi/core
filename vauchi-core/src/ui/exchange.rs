// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.

use crate::ui::*;

/// Configuration for starting an exchange.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExchangeConfig {
    pub own_name: String,
    pub own_qr_data: String,
}

/// Engine that drives the QR exchange workflow.
pub struct ExchangeEngine {
    step: ExchangeStep,
    config: ExchangeConfig,
    scanned_data: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum ExchangeStep {
    ShowQr,
    ScanQr,
    Verifying,
    Success,
    Failed,
}

impl ExchangeStep {
    fn step_number(&self) -> u8 {
        match self {
            Self::ShowQr => 1,
            Self::ScanQr => 2,
            Self::Verifying => 3,
            Self::Success => 4,
            Self::Failed => 5,
        }
    }
}

const TOTAL_STEPS: u8 = 5;

impl ExchangeEngine {
    pub fn new(config: ExchangeConfig) -> Self {
        Self {
            step: ExchangeStep::ShowQr,
            config,
            scanned_data: None,
        }
    }

    /// Mark the exchange as successfully verified (called by the caller after
    /// crypto verification completes).
    pub fn mark_success(&mut self) {
        self.step = ExchangeStep::Success;
    }

    /// Mark the exchange as failed (called by the caller when verification
    /// fails).
    pub fn mark_failed(&mut self) {
        self.step = ExchangeStep::Failed;
    }

    /// Returns the data scanned from the peer's QR code, if any.
    pub fn scanned_data(&self) -> Option<&str> {
        self.scanned_data.as_deref()
    }

    fn progress(&self) -> Progress {
        Progress {
            current_step: self.step.step_number(),
            total_steps: TOTAL_STEPS,
            label: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            ExchangeStep::ShowQr => ScreenModel {
                screen_id: "exchange_show_qr".to_string(),
                title: "Share Your Code".to_string(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "own_qr".to_string(),
                    data: self.config.own_qr_data.clone(),
                    mode: QrMode::Display,
                    label: Some(self.config.own_name.clone()),
                }],
                actions: vec![ScreenAction {
                    id: "continue".to_string(),
                    label: "Scan Their Code".to_string(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::ScanQr => ScreenModel {
                screen_id: "exchange_scan_qr".to_string(),
                title: "Scan Their Code".to_string(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "scan_qr".to_string(),
                    data: String::new(),
                    mode: QrMode::Scan,
                    label: None,
                }],
                actions: vec![ScreenAction {
                    id: "back".to_string(),
                    label: "Back".to_string(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::Verifying => ScreenModel {
                screen_id: "exchange_verifying".to_string(),
                title: "Verifying".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "verifying_status".to_string(),
                    icon: None,
                    title: "Verifying...".to_string(),
                    detail: None,
                    status: Status::InProgress,
                }],
                actions: vec![],
                progress: Some(self.progress()),
            },
            ExchangeStep::Success => ScreenModel {
                screen_id: "exchange_success".to_string(),
                title: "Success".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "success_status".to_string(),
                    icon: None,
                    title: "Exchange Complete".to_string(),
                    detail: None,
                    status: Status::Success,
                }],
                actions: vec![ScreenAction {
                    id: "done".to_string(),
                    label: "Done".to_string(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::Failed => ScreenModel {
                screen_id: "exchange_failed".to_string(),
                title: "Failed".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "failed_status".to_string(),
                    icon: None,
                    title: "Exchange Failed".to_string(),
                    detail: None,
                    status: Status::Failed,
                }],
                actions: vec![
                    ScreenAction {
                        id: "retry".to_string(),
                        label: "Retry".to_string(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".to_string(),
                        label: "Cancel".to_string(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: Some(self.progress()),
            },
        }
    }
}

impl WorkflowEngine for ExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            (ExchangeStep::ShowQr, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                self.step = ExchangeStep::ScanQr;
                ActionResult::RequestCamera
            }
            (ExchangeStep::ScanQr, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (
                ExchangeStep::ScanQr,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "scanned_data" => {
                self.scanned_data = Some(value);
                self.step = ExchangeStep::Verifying;
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Success, UserAction::ActionPressed { action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "retry" =>
            {
                self.scanned_data = None;
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
