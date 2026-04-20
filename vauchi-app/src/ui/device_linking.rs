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
                screen_id: "link_show_qr".into(),
                title: "Link Device".into(),
                subtitle: None,
                components: vec![
                    Component::QrCode {
                        id: "qr".into(),
                        data: self.qr_data.clone(),
                        mode: QrMode::Display,
                        label: Some("Scan on new device".into()),
                        scan_quality: None,
                        a11y: Some(A11y {
                            label: Some("Device link QR code".into()),
                            hint: Some(
                                "Scan this code on your new device to begin linking.".into(),
                            ),
                            role: Some(AccessibilityRole::Image),
                        }),
                    },
                    Component::Text {
                        id: "join_hint".into(),
                        content: "To join from another device, use: vauchi device join <qr_data>"
                            .into(),
                        style: TextStyle::Caption,
                    },
                ],
                actions: vec![ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::VerifyCode => {
                let code = self.verification_code.as_deref().unwrap_or("------");
                ScreenModel {
                    screen_id: "link_verify".into(),
                    title: "Verify Device".into(),
                    subtitle: None,
                    components: vec![
                        Component::Text {
                            id: "code".into(),
                            content: code.to_string(),
                            style: TextStyle::Title,
                        },
                        Component::InfoPanel {
                            id: "verify_info".into(),
                            icon: Some("shield".into()),
                            title: "Verify this code".into(),
                            items: vec![InfoItem {
                                icon: None,
                                title: "Compare codes".into(),
                                detail: "Ensure both devices show the same code".into(),
                            }],
                            a11y: None,
                        },
                    ],
                    actions: vec![
                        ScreenAction {
                            id: "confirm".into(),
                            label: "Confirm".into(),
                            style: ActionStyle::Primary,
                            enabled: true,
                            a11y: None,
                        },
                        ScreenAction {
                            id: "reject".into(),
                            label: "Reject".into(),
                            style: ActionStyle::Destructive,
                            enabled: true,
                            a11y: None,
                        },
                    ],
                    progress: self.progress(),
                    ..Default::default()
                }
            }
            DeviceLinkStep::Syncing => ScreenModel {
                screen_id: "link_syncing".into(),
                title: "Syncing".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "syncing".into(),
                    icon: None,
                    title: "Syncing data...".into(),
                    detail: None,
                    status: Status::InProgress,
                    a11y: Some(A11y {
                        label: Some("Syncing data status".into()),
                        hint: Some("Data is being synced to the new device.".into()),
                        role: None,
                    }),
                }],
                actions: vec![],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::Complete => ScreenModel {
                screen_id: "link_complete".into(),
                title: "Device Linked".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "complete".into(),
                    icon: None,
                    title: "Device Linked".into(),
                    detail: None,
                    status: Status::Success,
                    a11y: Some(A11y {
                        label: Some("Device Linked status".into()),
                        hint: Some("Your new device has been linked successfully.".into()),
                        role: None,
                    }),
                }],
                actions: vec![ScreenAction {
                    id: "done".into(),
                    label: "Done".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
        }
    }
}

impl WorkflowEngine for DeviceLinkingEngine {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

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
