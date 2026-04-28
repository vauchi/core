// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking engine — guides the user through linking a new device.
//!
//! Pair 5 of the Pure Humble UI retirement work
//! (`_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use crate::ui::*;

/// Steps in the device linking flow.
#[derive(Clone, Debug, PartialEq)]
enum DeviceLinkStep {
    /// Pre-step: user picks the transport (Internet relay vs offline
    /// multipart-QR). Added 2026-04-28 to lift the bespoke iOS
    /// `DeviceLinkSheet.transportSelectionView` into core.
    TransportSelection,
    /// Stub for the offline / multipart-QR transport — not yet
    /// implemented in core. Renders an info panel explaining that
    /// the offline path is not available.
    OfflineStub,
    ShowQr,
    VerifyCode,
    Syncing,
    Complete,
}

/// Transport options for the device-link selector.
pub const TRANSPORT_INTERNET_ACTION_ID: &str = "select_internet";
pub const TRANSPORT_OFFLINE_ACTION_ID: &str = "select_offline";

/// Engine that drives the device linking workflow.
#[derive(Clone, Debug)]
pub struct DeviceLinkingEngine {
    step: DeviceLinkStep,
    qr_data: String,
    verification_code: Option<String>,
}

impl DeviceLinkingEngine {
    /// Creates a new engine starting at the QR display step.
    ///
    /// Backwards-compatible entry point for the existing `Settings`
    /// "Link New Device" path on Android / linux-gtk / TUI which skips
    /// the transport picker and goes straight to the relay (Internet)
    /// flow. iOS / macOS use [`Self::with_transport_selection`] to keep
    /// their two-stage UX (transport picker → QR display).
    pub fn new(qr_data: String) -> Self {
        Self {
            step: DeviceLinkStep::ShowQr,
            qr_data,
            verification_code: None,
        }
    }

    /// Creates a new engine starting at the transport-selection step.
    /// The QR data is captured up-front but only revealed after the
    /// user picks the Internet transport.
    pub fn with_transport_selection(qr_data: String) -> Self {
        Self {
            step: DeviceLinkStep::TransportSelection,
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
            DeviceLinkStep::TransportSelection => 0,
            DeviceLinkStep::OfflineStub => 0,
            DeviceLinkStep::ShowQr => 1,
            DeviceLinkStep::VerifyCode => 2,
            DeviceLinkStep::Syncing => 3,
            DeviceLinkStep::Complete => 4,
        }
    }

    fn progress(&self) -> Option<Progress> {
        // No progress shown on the pre-flow steps.
        if matches!(
            self.step,
            DeviceLinkStep::TransportSelection | DeviceLinkStep::OfflineStub
        ) {
            return None;
        }
        Some(Progress {
            current_step: self.step_number(),
            total_steps: 4,
            label: None,
        })
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            DeviceLinkStep::TransportSelection => ScreenModel {
                screen_id: "link_transport".into(),
                title: "Link New Device".into(),
                subtitle: Some("How would you like to link?".into()),
                components: vec![Component::InfoPanel {
                    id: "link_transport_info".into(),
                    icon: Some("link".into()),
                    title: "Choose how to connect with your new device.".into(),
                    items: vec![
                        InfoItem {
                            icon: Some("wifi".into()),
                            title: "Link via Internet".into(),
                            detail: "Uses the relay server over the network.".into(),
                        },
                        InfoItem {
                            icon: Some("qrcode".into()),
                            title: "Link Offline (multipart QR)".into(),
                            detail: "Coming soon — shows a stub for now.".into(),
                        },
                    ],
                    a11y: Some(A11y {
                        label: Some("Device link transport selection".into()),
                        hint: Some("Pick a transport to start the device link flow.".into()),
                        role: Some(AccessibilityRole::Heading),
                    }),
                }],
                actions: vec![
                    ScreenAction {
                        id: TRANSPORT_INTERNET_ACTION_ID.into(),
                        label: "Link via Internet".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: TRANSPORT_OFFLINE_ACTION_ID.into(),
                        label: "Link Offline".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::OfflineStub => ScreenModel {
                screen_id: "link_offline_stub".into(),
                title: "Offline Linking".into(),
                subtitle: None,
                components: vec![Component::InfoPanel {
                    id: "offline_stub".into(),
                    icon: Some("info".into()),
                    title: "Offline linking is not yet available".into(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Use Internet linking for now".into(),
                        detail: "Multipart-QR offline linking ships in a future release.".into(),
                    }],
                    a11y: None,
                }],
                actions: vec![
                    ScreenAction {
                        id: "back_to_transport".into(),
                        label: "Back".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: "cancel".into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: self.progress(),
                ..Default::default()
            },
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
                DeviceLinkStep::TransportSelection => match action_id.as_str() {
                    TRANSPORT_INTERNET_ACTION_ID => {
                        self.step = DeviceLinkStep::ShowQr;
                        ActionResult::NavigateTo(self.build_screen())
                    }
                    TRANSPORT_OFFLINE_ACTION_ID => {
                        self.step = DeviceLinkStep::OfflineStub;
                        ActionResult::NavigateTo(self.build_screen())
                    }
                    "cancel" => ActionResult::Complete,
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                },
                DeviceLinkStep::OfflineStub => match action_id.as_str() {
                    "back_to_transport" => {
                        self.step = DeviceLinkStep::TransportSelection;
                        ActionResult::NavigateTo(self.build_screen())
                    }
                    "cancel" => ActionResult::Complete,
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                },
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

// INLINE_TEST_REQUIRED: covers private DeviceLinkStep transitions and
// the transport-selection state machine added 2026-04-28. Cross-crate
// integration tests live elsewhere.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn new_starts_at_show_qr_for_backwards_compat() {
        let e = DeviceLinkingEngine::new("qr-data".into());
        assert_eq!(e.current_screen().screen_id, "link_show_qr");
    }

    // @internal
    #[test]
    fn with_transport_selection_starts_at_transport_picker() {
        let e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_transport");
        assert_eq!(screen.actions.len(), 3); // internet + offline + cancel
        assert_eq!(screen.actions[0].id, TRANSPORT_INTERNET_ACTION_ID);
        assert_eq!(screen.actions[1].id, TRANSPORT_OFFLINE_ACTION_ID);
    }

    // @internal
    #[test]
    fn select_internet_advances_to_show_qr() {
        let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: TRANSPORT_INTERNET_ACTION_ID.into(),
        });
        match result {
            ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_show_qr"),
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn select_offline_advances_to_offline_stub() {
        let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: TRANSPORT_OFFLINE_ACTION_ID.into(),
        });
        match result {
            ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_offline_stub"),
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn back_from_offline_returns_to_transport() {
        let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: TRANSPORT_OFFLINE_ACTION_ID.into(),
        });
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "back_to_transport".into(),
        });
        match result {
            ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_transport"),
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn cancel_from_transport_emits_complete() {
        let mut e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
    }

    // @internal
    #[test]
    fn peer_connected_advances_show_qr_to_verify_code() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.peer_connected("123456".into());
        assert_eq!(e.current_screen().screen_id, "link_verify");
        // Verification code rendered as Text content
        if let Component::Text { content, .. } = &e.current_screen().components[0] {
            assert_eq!(content, "123456");
        } else {
            panic!("expected Text component");
        }
    }

    // @internal
    #[test]
    fn confirm_from_verify_advances_to_syncing() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.peer_connected("123456".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        match result {
            ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_syncing"),
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn reject_from_verify_returns_to_show_qr() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.peer_connected("123456".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "reject".into(),
        });
        match result {
            ActionResult::NavigateTo(s) => assert_eq!(s.screen_id, "link_show_qr"),
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn sync_complete_advances_to_complete() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.peer_connected("123456".into());
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        e.sync_complete();
        assert_eq!(e.current_screen().screen_id, "link_complete");
    }

    // @internal
    #[test]
    fn progress_hidden_on_pre_flow_steps() {
        let e = DeviceLinkingEngine::with_transport_selection("qr-data".into());
        assert!(e.current_screen().progress.is_none());
    }
}
