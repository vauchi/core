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
    /// QR code is being generated; no QR data yet. Pair 5 receiver-side
    /// retirement (2026-04-29).
    QrPending,
    ShowQr,
    /// QR code is displayed and the engine is waiting for a peer to
    /// scan it; carries the absolute unix-seconds expiry so the
    /// frontend can render a countdown.
    WaitingForRequest {
        expires_at: u64,
    },
    /// QR code window expired before any peer connected.
    QrExpired,
    /// Legacy verify-code state: the simpler Android/CLI path that uses
    /// only the verification code. Kept for backwards compatibility
    /// with `peer_connected`.
    VerifyCode,
    /// Receiver-side: a peer connected and we now show the device
    /// name, the confirmation code, and a hex-encoded challenge that
    /// the next step (`VerifyingProximity`) will sign.
    ConfirmingDevice {
        device_name: String,
        code: String,
        challenge_hex: String,
    },
    /// Receiver-side proximity verification. The user confirms manually
    /// (ultrasonic-approve flow is deferred — see ADR-031 hardware
    /// events).
    VerifyingProximity {
        code: String,
        challenge_hex: String,
    },
    Syncing,
    /// Sending credentials to the new device; ephemeral progress state
    /// between proximity confirmation and final success.
    Completing,
    Complete,
    /// Linking failed; carries the error message to render.
    LinkFailed {
        message: String,
    },
}

/// Transport options for the device-link selector.
pub const TRANSPORT_INTERNET_ACTION_ID: &str = "select_internet";
pub const TRANSPORT_OFFLINE_ACTION_ID: &str = "select_offline";

/// Action ids handled by `DeviceLinkingEngine`. Extracted for the
/// reachability test (`tests/reachability/device_linking.rs`) so the
/// declared handler set can't drift from the live ScreenModel.
pub const BACK_TO_TRANSPORT_ACTION_ID: &str = "back_to_transport";
pub const CANCEL_ACTION_ID: &str = "cancel";
pub const CONFIRM_ACTION_ID: &str = "confirm";
pub const REJECT_ACTION_ID: &str = "reject";
pub const DONE_ACTION_ID: &str = "done";
pub const CODES_MATCH_ACTION_ID: &str = "codes_match";
pub const DENY_ACTION_ID: &str = "deny";
pub const CONFIRM_MANUAL_ACTION_ID: &str = "confirm_manual";
pub const RETRY_ACTION_ID: &str = "retry";

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

    /// Receiver-side bridge: enter the QR-pending state while the
    /// transport prepares the device-link payload.
    pub fn transition_to_qr_pending(&mut self) {
        self.step = DeviceLinkStep::QrPending;
    }

    /// Receiver-side bridge: the QR is ready and the engine is waiting
    /// for a peer to scan it. `expires_at` is unix-seconds; the frontend
    /// uses it to render a countdown (5-min window per ADR-035).
    pub fn transition_to_waiting_for_request(&mut self, qr_data: String, expires_at: u64) {
        self.qr_data = qr_data;
        self.step = DeviceLinkStep::WaitingForRequest { expires_at };
    }

    /// Receiver-side bridge: the QR window expired before any peer
    /// connected. The user can hit `retry` to regenerate.
    pub fn transition_to_qr_expired(&mut self) {
        self.step = DeviceLinkStep::QrExpired;
    }

    /// Receiver-side bridge: a peer device wants to link. Show the
    /// device name + confirmation code, hold the challenge for the
    /// proximity step.
    pub fn transition_to_confirming_device(
        &mut self,
        device_name: String,
        code: String,
        challenge_hex: String,
    ) {
        self.step = DeviceLinkStep::ConfirmingDevice {
            device_name,
            code,
            challenge_hex,
        };
    }

    /// Receiver-side bridge: proximity has been confirmed manually and
    /// the engine is now finalizing credentials transmission. Ephemeral
    /// state, replaced by either `Complete` or `LinkFailed`.
    pub fn transition_to_completing(&mut self) {
        self.step = DeviceLinkStep::Completing;
    }

    /// Receiver-side bridge: the device link succeeded. Equivalent to
    /// `sync_complete` but reachable from any non-terminal step (the
    /// completing state is not always preceded by `Syncing`).
    pub fn transition_to_link_success(&mut self) {
        self.step = DeviceLinkStep::Complete;
    }

    /// Receiver-side bridge: the device link failed. `message` is
    /// rendered to the user; the only follow-up actions are `retry`
    /// (back to QR generation) or `cancel` (abort).
    pub fn transition_to_link_failed(&mut self, message: String) {
        self.step = DeviceLinkStep::LinkFailed { message };
    }

    fn step_number(&self) -> u8 {
        match &self.step {
            DeviceLinkStep::TransportSelection
            | DeviceLinkStep::OfflineStub
            | DeviceLinkStep::QrExpired
            | DeviceLinkStep::LinkFailed { .. } => 0,
            DeviceLinkStep::QrPending
            | DeviceLinkStep::ShowQr
            | DeviceLinkStep::WaitingForRequest { .. } => 1,
            DeviceLinkStep::VerifyCode
            | DeviceLinkStep::ConfirmingDevice { .. }
            | DeviceLinkStep::VerifyingProximity { .. } => 2,
            DeviceLinkStep::Syncing | DeviceLinkStep::Completing => 3,
            DeviceLinkStep::Complete => 4,
        }
    }

    fn progress(&self) -> Option<Progress> {
        // No progress shown on the pre-flow steps or terminal-error
        // states (they have their own affordances rather than a
        // numbered-step indicator).
        if matches!(
            &self.step,
            DeviceLinkStep::TransportSelection
                | DeviceLinkStep::OfflineStub
                | DeviceLinkStep::QrExpired
                | DeviceLinkStep::LinkFailed { .. }
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
                        id: CANCEL_ACTION_ID.into(),
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
                        id: BACK_TO_TRANSPORT_ACTION_ID.into(),
                        label: "Back".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
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
                    id: CANCEL_ACTION_ID.into(),
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
                            id: CONFIRM_ACTION_ID.into(),
                            label: "Confirm".into(),
                            style: ActionStyle::Primary,
                            enabled: true,
                            a11y: None,
                        },
                        ScreenAction {
                            id: REJECT_ACTION_ID.into(),
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
                    id: DONE_ACTION_ID.into(),
                    label: "Done".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::QrPending => ScreenModel {
                screen_id: "link_qr_pending".into(),
                title: "Link Device".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "qr_pending".into(),
                    icon: None,
                    title: "Generating link...".into(),
                    detail: None,
                    status: Status::InProgress,
                    a11y: Some(A11y {
                        label: Some("Generating device link".into()),
                        hint: Some("Preparing the QR code for the new device.".into()),
                        role: None,
                    }),
                }],
                actions: vec![ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::WaitingForRequest { expires_at } => ScreenModel {
                screen_id: "link_waiting".into(),
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
                        id: "expires_at".into(),
                        content: format!("Expires at {expires_at}"),
                        style: TextStyle::Caption,
                    },
                ],
                actions: vec![ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::QrExpired => ScreenModel {
                screen_id: "link_qr_expired".into(),
                title: "QR Code Expired".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "qr_expired".into(),
                    icon: Some("clock".into()),
                    title: "QR code expired".into(),
                    detail: Some("Generate a new code to continue linking.".into()),
                    status: Status::Warning,
                    a11y: Some(A11y {
                        label: Some("Device link QR expired".into()),
                        hint: Some(
                            "The 5-minute QR window elapsed. Retry to generate a new code.".into(),
                        ),
                        role: None,
                    }),
                }],
                actions: vec![
                    ScreenAction {
                        id: RETRY_ACTION_ID.into(),
                        label: "Generate New QR".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::ConfirmingDevice {
                device_name, code, ..
            } => ScreenModel {
                screen_id: "link_confirming_device".into(),
                title: "Device Wants to Link".into(),
                subtitle: Some(format!("Device: {device_name}")),
                components: vec![
                    Component::Text {
                        id: "code".into(),
                        content: code.clone(),
                        style: TextStyle::Title,
                    },
                    Component::InfoPanel {
                        id: "confirm_device_info".into(),
                        icon: Some("shield".into()),
                        title: "Verify this code matches the new device".into(),
                        items: vec![InfoItem {
                            icon: None,
                            title: "Compare codes".into(),
                            detail: "Both devices must show the same code before proceeding."
                                .into(),
                        }],
                        a11y: None,
                    },
                ],
                actions: vec![
                    ScreenAction {
                        id: CODES_MATCH_ACTION_ID.into(),
                        label: "Codes Match — Verify Proximity".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: DENY_ACTION_ID.into(),
                        label: "Deny".into(),
                        style: ActionStyle::Destructive,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::VerifyingProximity { code, .. } => ScreenModel {
                screen_id: "link_verifying_proximity".into(),
                title: "Verify Proximity".into(),
                subtitle: None,
                components: vec![
                    Component::Text {
                        id: "code".into(),
                        content: code.clone(),
                        style: TextStyle::Title,
                    },
                    Component::InfoPanel {
                        id: "proximity_info".into(),
                        icon: Some("wave.3.right".into()),
                        title: "Confirm the new device is near you".into(),
                        items: vec![InfoItem {
                            icon: None,
                            title: "Manual confirmation".into(),
                            detail: "Tap Confirm once you can see the same code on the new device."
                                .into(),
                        }],
                        a11y: None,
                    },
                ],
                actions: vec![
                    ScreenAction {
                        id: CONFIRM_MANUAL_ACTION_ID.into(),
                        label: "Confirm".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::Completing => ScreenModel {
                screen_id: "link_completing".into(),
                title: "Completing Link".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "completing".into(),
                    icon: None,
                    title: "Sending credentials...".into(),
                    detail: Some("Transferring identity to the new device.".into()),
                    status: Status::InProgress,
                    a11y: Some(A11y {
                        label: Some("Completing device link".into()),
                        hint: Some("Sending credentials to the new device.".into()),
                        role: None,
                    }),
                }],
                actions: vec![ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                }],
                progress: self.progress(),
                ..Default::default()
            },
            DeviceLinkStep::LinkFailed { message } => ScreenModel {
                screen_id: "link_failed".into(),
                title: "Linking Failed".into(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "link_failed".into(),
                    icon: Some("exclamationmark.triangle".into()),
                    title: "Linking failed".into(),
                    detail: Some(message.clone()),
                    status: Status::Failed,
                    a11y: Some(A11y {
                        label: Some("Device link failed".into()),
                        hint: Some("The device link could not be completed.".into()),
                        role: None,
                    }),
                }],
                actions: vec![
                    ScreenAction {
                        id: RETRY_ACTION_ID.into(),
                        label: "Try Again".into(),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: None,
                    },
                    ScreenAction {
                        id: CANCEL_ACTION_ID.into(),
                        label: "Cancel".into(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: None,
                    },
                ],
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
        let UserAction::ActionPressed { action_id } = action else {
            return ActionResult::UpdateScreen(self.build_screen());
        };
        let id = action_id.as_str();
        match (&self.step, id) {
            (DeviceLinkStep::TransportSelection, TRANSPORT_INTERNET_ACTION_ID) => {
                self.step = DeviceLinkStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (DeviceLinkStep::TransportSelection, TRANSPORT_OFFLINE_ACTION_ID) => {
                self.step = DeviceLinkStep::OfflineStub;
                ActionResult::NavigateTo(self.build_screen())
            }
            (DeviceLinkStep::OfflineStub, BACK_TO_TRANSPORT_ACTION_ID) => {
                self.step = DeviceLinkStep::TransportSelection;
                ActionResult::NavigateTo(self.build_screen())
            }
            (DeviceLinkStep::VerifyCode, CONFIRM_ACTION_ID) => {
                self.step = DeviceLinkStep::Syncing;
                ActionResult::NavigateTo(self.build_screen())
            }
            (DeviceLinkStep::VerifyCode, REJECT_ACTION_ID) => {
                self.step = DeviceLinkStep::ShowQr;
                self.verification_code = None;
                ActionResult::NavigateTo(self.build_screen())
            }
            (
                DeviceLinkStep::ConfirmingDevice {
                    code,
                    challenge_hex,
                    ..
                },
                CODES_MATCH_ACTION_ID,
            ) => {
                let code = code.clone();
                let challenge_hex = challenge_hex.clone();
                self.step = DeviceLinkStep::VerifyingProximity {
                    code,
                    challenge_hex,
                };
                ActionResult::NavigateTo(self.build_screen())
            }
            // `deny` from receiver-side ConfirmingDevice. The app
            // engine intercepts `DeviceLinkDeny` to call
            // `MobileDeviceLinkSession::deny`; the cycle thread
            // emits `on_failed("user_denied")` + `on_session_ended()`
            // which collapses the sheet.
            (DeviceLinkStep::ConfirmingDevice { .. }, DENY_ACTION_ID) => {
                ActionResult::DeviceLinkDeny
            }
            // `confirm_manual` from VerifyingProximity. Engine moves
            // to the ephemeral Completing state and emits the typed
            // result so the app engine can call
            // `MobileDeviceLinkSession::confirm_manual(code, now)`.
            (DeviceLinkStep::VerifyingProximity { code, .. }, CONFIRM_MANUAL_ACTION_ID) => {
                let code = code.clone();
                self.step = DeviceLinkStep::Completing;
                ActionResult::DeviceLinkConfirmManual { code }
            }
            (DeviceLinkStep::QrExpired, RETRY_ACTION_ID)
            | (DeviceLinkStep::LinkFailed { .. }, RETRY_ACTION_ID) => {
                self.step = DeviceLinkStep::QrPending;
                self.verification_code = None;
                ActionResult::DeviceLinkRetry
            }
            (DeviceLinkStep::Complete, DONE_ACTION_ID) => ActionResult::Complete,
            // `cancel` is universal across every screen that shows it.
            (_, CANCEL_ACTION_ID) => ActionResult::Complete,
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

    // ---- Pair 5 receiver-side state coverage ----

    // @internal
    #[test]
    fn transition_to_qr_pending_sets_pending_screen() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_qr_pending();
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_qr_pending");
        assert_eq!(screen.actions.len(), 1);
        assert_eq!(screen.actions[0].id, CANCEL_ACTION_ID);
    }

    // @internal
    #[test]
    fn transition_to_waiting_renders_qr_with_expiry() {
        let mut e = DeviceLinkingEngine::new("old".into());
        e.transition_to_waiting_for_request("new-qr-data".into(), 1_700_000_500);
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_waiting");
        let qr = screen
            .components
            .iter()
            .find(|c| matches!(c, Component::QrCode { .. }))
            .expect("QR component present");
        if let Component::QrCode { data, .. } = qr {
            assert_eq!(data, "new-qr-data");
        }
        let expiry = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::Text { id, content, .. } if id == "expires_at" => Some(content.clone()),
                _ => None,
            })
            .expect("expires_at text present");
        assert!(expiry.contains("1700000500"));
    }

    // @internal
    #[test]
    fn transition_to_qr_expired_shows_retry_and_cancel() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_qr_expired();
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_qr_expired");
        let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
    }

    // @internal
    #[test]
    fn confirming_device_screen_shows_name_and_code() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_confirming_device");
        assert_eq!(screen.subtitle.as_deref(), Some("Device: New iPad"));
        let code = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::Text { content, .. } => Some(content.clone()),
                _ => None,
            })
            .expect("code text present");
        assert_eq!(code, "654321");
        let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec![CODES_MATCH_ACTION_ID, DENY_ACTION_ID]);
    }

    // @internal
    #[test]
    fn codes_match_advances_to_verifying_proximity_preserving_code() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: CODES_MATCH_ACTION_ID.into(),
        });
        match result {
            ActionResult::NavigateTo(s) => {
                assert_eq!(s.screen_id, "link_verifying_proximity");
                let code = s
                    .components
                    .iter()
                    .find_map(|c| match c {
                        Component::Text { content, .. } => Some(content.clone()),
                        _ => None,
                    })
                    .expect("code text present");
                assert_eq!(code, "654321");
            }
            other => panic!("expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn deny_from_confirming_device_emits_device_link_deny() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: DENY_ACTION_ID.into(),
        });
        assert!(
            matches!(result, ActionResult::DeviceLinkDeny),
            "expected DeviceLinkDeny, got {result:?}"
        );
    }

    // @internal
    #[test]
    fn confirm_manual_emits_typed_result_with_code_and_advances_step() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
        let _ = e.handle_action(UserAction::ActionPressed {
            action_id: CODES_MATCH_ACTION_ID.into(),
        });
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: CONFIRM_MANUAL_ACTION_ID.into(),
        });
        match result {
            ActionResult::DeviceLinkConfirmManual { code } => assert_eq!(code, "654321"),
            other => panic!("expected DeviceLinkConfirmManual, got {other:?}"),
        }
        // Step still advanced — next render shows the Completing screen.
        assert_eq!(e.current_screen().screen_id, "link_completing");
    }

    // @internal
    #[test]
    fn transition_to_completing_uses_completing_screen() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_confirming_device("New iPad".into(), "654321".into(), "deadbeef".into());
        e.transition_to_completing();
        assert_eq!(e.current_screen().screen_id, "link_completing");
    }

    // @internal
    #[test]
    fn transition_to_link_success_uses_complete_screen() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_link_success();
        assert_eq!(e.current_screen().screen_id, "link_complete");
    }

    // @internal
    #[test]
    fn transition_to_link_failed_renders_message() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_link_failed("relay unreachable".into());
        let screen = e.current_screen();
        assert_eq!(screen.screen_id, "link_failed");
        let detail = screen
            .components
            .iter()
            .find_map(|c| match c {
                Component::StatusIndicator { detail, .. } => detail.clone(),
                _ => None,
            })
            .expect("status detail present");
        assert_eq!(detail, "relay unreachable");
        let ids: Vec<&str> = screen.actions.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, vec![RETRY_ACTION_ID, CANCEL_ACTION_ID]);
    }

    // @internal
    #[test]
    fn retry_from_qr_expired_emits_device_link_retry_and_advances_step() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_qr_expired();
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: RETRY_ACTION_ID.into(),
        });
        assert!(
            matches!(result, ActionResult::DeviceLinkRetry),
            "expected DeviceLinkRetry, got {result:?}"
        );
        assert_eq!(e.current_screen().screen_id, "link_qr_pending");
    }

    // @internal
    #[test]
    fn retry_from_link_failed_emits_device_link_retry_and_advances_step() {
        let mut e = DeviceLinkingEngine::new("qr-data".into());
        e.transition_to_link_failed("oops".into());
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: RETRY_ACTION_ID.into(),
        });
        assert!(
            matches!(result, ActionResult::DeviceLinkRetry),
            "expected DeviceLinkRetry, got {result:?}"
        );
        assert_eq!(e.current_screen().screen_id, "link_qr_pending");
    }

    // @internal
    #[test]
    fn cancel_is_terminal_from_every_new_state() {
        for setup in [
            |e: &mut DeviceLinkingEngine| e.transition_to_qr_pending(),
            |e: &mut DeviceLinkingEngine| e.transition_to_waiting_for_request("qr".into(), 1),
            |e: &mut DeviceLinkingEngine| e.transition_to_qr_expired(),
            |e: &mut DeviceLinkingEngine| {
                e.transition_to_confirming_device("D".into(), "1".into(), "ab".into())
            },
            |e: &mut DeviceLinkingEngine| e.transition_to_completing(),
            |e: &mut DeviceLinkingEngine| e.transition_to_link_failed("x".into()),
        ] {
            let mut e = DeviceLinkingEngine::new("qr".into());
            setup(&mut e);
            let result = e.handle_action(UserAction::ActionPressed {
                action_id: CANCEL_ACTION_ID.into(),
            });
            assert!(
                matches!(result, ActionResult::Complete),
                "expected Complete, got {result:?}"
            );
        }
    }

    // @internal
    #[test]
    fn progress_hidden_on_qr_expired_and_link_failed() {
        let mut expired = DeviceLinkingEngine::new("qr".into());
        expired.transition_to_qr_expired();
        assert!(expired.current_screen().progress.is_none());

        let mut failed = DeviceLinkingEngine::new("qr".into());
        failed.transition_to_link_failed("x".into());
        assert!(failed.current_screen().progress.is_none());
    }
}
