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
            DeviceLinkStep::TransportSelection => self.transport_selection_screen(),
            DeviceLinkStep::OfflineStub => self.offline_stub_screen(),
            DeviceLinkStep::ShowQr => self.show_qr_screen(),
            DeviceLinkStep::VerifyCode => self.verify_code_screen(),
            DeviceLinkStep::Syncing => self.syncing_screen(),
            DeviceLinkStep::Complete => self.complete_screen(),
            DeviceLinkStep::QrPending => self.qr_pending_screen(),
            DeviceLinkStep::WaitingForRequest { expires_at } => {
                self.waiting_for_request_screen(expires_at)
            }
            DeviceLinkStep::QrExpired => self.qr_expired_screen(),
            DeviceLinkStep::ConfirmingDevice {
                device_name, code, ..
            } => self.confirming_device_screen(device_name, code),
            DeviceLinkStep::VerifyingProximity { code, .. } => {
                self.verifying_proximity_screen(code)
            }
            DeviceLinkStep::Completing => self.completing_screen(),
            DeviceLinkStep::LinkFailed { message } => self.link_failed_screen(message),
        }
    }

    fn transport_selection_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn offline_stub_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn show_qr_screen(&self) -> ScreenModel {
        ScreenModel {
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
                        hint: Some("Scan this code on your new device to begin linking.".into()),
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
        }
    }

    fn verify_code_screen(&self) -> ScreenModel {
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

    fn syncing_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn complete_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn qr_pending_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn waiting_for_request_screen(&self, expires_at: &u64) -> ScreenModel {
        ScreenModel {
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
                        hint: Some("Scan this code on your new device to begin linking.".into()),
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
        }
    }

    fn qr_expired_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn confirming_device_screen(&self, device_name: &str, code: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_confirming_device".into(),
            title: "Device Wants to Link".into(),
            subtitle: Some(format!("Device: {device_name}")),
            components: vec![
                Component::Text {
                    id: "code".into(),
                    content: code.to_string(),
                    style: TextStyle::Title,
                },
                Component::InfoPanel {
                    id: "confirm_device_info".into(),
                    icon: Some("shield".into()),
                    title: "Verify this code matches the new device".into(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Compare codes".into(),
                        detail: "Both devices must show the same code before proceeding.".into(),
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
        }
    }

    fn verifying_proximity_screen(&self, code: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_verifying_proximity".into(),
            title: "Verify Proximity".into(),
            subtitle: None,
            components: vec![
                Component::Text {
                    id: "code".into(),
                    content: code.to_string(),
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
        }
    }

    fn completing_screen(&self) -> ScreenModel {
        ScreenModel {
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
        }
    }

    fn link_failed_screen(&self, message: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_failed".into(),
            title: "Linking Failed".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "link_failed".into(),
                icon: Some("exclamationmark.triangle".into()),
                title: "Linking failed".into(),
                detail: Some(message.to_string()),
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
        }
    }
}

impl WorkflowEngine for DeviceLinkingEngine {
    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        use crate::ui::DeviceLinkUpdate as U;
        let crate::ui::EngineUpdate::DeviceLink(update) = update else {
            return false;
        };
        match update {
            U::QrPending => self.transition_to_qr_pending(),
            U::QrReady {
                qr_data,
                expires_at,
            } => self.transition_to_waiting_for_request(qr_data, expires_at),
            U::QrExpired => self.transition_to_qr_expired(),
            U::RequestReceived {
                device_name,
                confirmation_code,
                challenge_hex,
            } => {
                self.transition_to_confirming_device(device_name, confirmation_code, challenge_hex)
            }
            U::Completed => self.transition_to_link_success(),
            U::Failed(reason) => self.transition_to_link_failed(reason),
        }
        true
    }

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
#[path = "device_linking_tests.rs"]
mod tests;
