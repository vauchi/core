// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking engine — guides the user through linking a new device.

use crate::ui::*;

/// Steps in the device linking flow.
#[derive(Clone, Debug, PartialEq)]
enum DeviceLinkStep {
    ShowQr,
    VerifyCode,
    Syncing,
    Complete,
}

/// Engine that drives the device linking workflow.
#[derive(Clone, Debug)]
pub struct DeviceLinkingEngine {
    step: DeviceLinkStep,
    qr_data: String,
    verification_code: Option<String>,
}

impl DeviceLinkingEngine {
    /// Creates a new engine starting at the QR display step.
    pub fn new(qr_data: String) -> Self {
        Self {
            step: DeviceLinkStep::ShowQr,
            qr_data,
            verification_code: None,
        }
    }

    /// Signal that a peer device has connected, providing the verification code.
    ///
    /// Transitions from `ShowQr` to `VerifyCode`.
    pub fn peer_connected(&mut self, verification_code: String) {
        if self.step == DeviceLinkStep::ShowQr {
            self.verification_code = Some(verification_code);
            self.step = DeviceLinkStep::VerifyCode;
        }
    }

    /// Signal that data sync has completed.
    pub fn sync_complete(&mut self) {
        if self.step == DeviceLinkStep::Syncing {
            self.step = DeviceLinkStep::Complete;
        }
    }

    fn step_number(&self) -> u8 {
        match self.step {
            DeviceLinkStep::ShowQr => 1,
            DeviceLinkStep::VerifyCode => 2,
            DeviceLinkStep::Syncing => 3,
            DeviceLinkStep::Complete => 4,
        }
    }

    fn progress(&self) -> Option<Progress> {
        Some(Progress {
            current_step: self.step_number(),
            total_steps: 4,
            label: None,
        })
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            DeviceLinkStep::ShowQr => ScreenModel {
                screen_id: "link_show_qr".to_string(),
                title: "Link Device".to_string(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "qr".to_string(),
                    data: self.qr_data.clone(),
                    mode: QrMode::Display,
                    label: Some("Scan on new device".to_string()),
                }],
                actions: vec![ScreenAction {
                    id: "cancel".to_string(),
                    label: "Cancel".to_string(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                }],
                progress: self.progress(),
            },
            DeviceLinkStep::VerifyCode => {
                let code = self.verification_code.as_deref().unwrap_or("------");
                ScreenModel {
                    screen_id: "link_verify".to_string(),
                    title: "Verify Device".to_string(),
                    subtitle: None,
                    components: vec![
                        Component::Text {
                            id: "code".to_string(),
                            content: code.to_string(),
                            style: TextStyle::Title,
                        },
                        Component::InfoPanel {
                            id: "verify_info".to_string(),
                            icon: Some("shield".to_string()),
                            title: "Verify this code".to_string(),
                            items: vec![InfoItem {
                                icon: None,
                                title: "Compare codes".to_string(),
                                detail: "Ensure both devices show the same code".to_string(),
                            }],
                        },
                    ],
                    actions: vec![
                        ScreenAction {
                            id: "confirm".to_string(),
                            label: "Confirm".to_string(),
                            style: ActionStyle::Primary,
                            enabled: true,
                        },
                        ScreenAction {
                            id: "reject".to_string(),
                            label: "Reject".to_string(),
                            style: ActionStyle::Destructive,
                            enabled: true,
                        },
                    ],
                    progress: self.progress(),
                }
            }
            DeviceLinkStep::Syncing => ScreenModel {
                screen_id: "link_syncing".to_string(),
                title: "Syncing".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "syncing".to_string(),
                    icon: None,
                    title: "Syncing data...".to_string(),
                    detail: None,
                    status: Status::InProgress,
                }],
                actions: vec![],
                progress: self.progress(),
            },
            DeviceLinkStep::Complete => ScreenModel {
                screen_id: "link_complete".to_string(),
                title: "Device Linked".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "complete".to_string(),
                    icon: None,
                    title: "Device Linked".to_string(),
                    detail: None,
                    status: Status::Success,
                }],
                actions: vec![ScreenAction {
                    id: "done".to_string(),
                    label: "Done".to_string(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: self.progress(),
            },
        }
    }
}

impl WorkflowEngine for DeviceLinkingEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match self.step {
                DeviceLinkStep::ShowQr if action_id == "cancel" => ActionResult::Complete,
                DeviceLinkStep::VerifyCode if action_id == "confirm" => {
                    self.step = DeviceLinkStep::Syncing;
                    ActionResult::NavigateTo(self.build_screen())
                }
                DeviceLinkStep::VerifyCode if action_id == "reject" => {
                    self.step = DeviceLinkStep::ShowQr;
                    self.verification_code = None;
                    ActionResult::NavigateTo(self.build_screen())
                }
                DeviceLinkStep::Complete if action_id == "done" => ActionResult::Complete,
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
