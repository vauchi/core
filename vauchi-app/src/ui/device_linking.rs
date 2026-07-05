// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking engine — guides the user through linking a new device.
//!
//! Pair 5 of the Pure Humble UI retirement work
//! (`_private/docs/problems/2026-04-28-pure-humble-ui-retire-native-screens/`).

use crate::i18n::{Locale, get_string, get_string_with_args};
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
    /// name and the confirmation code. The user confirms the codes
    /// match; that single confirmation completes the link (M5 B2b —
    /// 2026-07-03-second-device-join-dead-end item 5 collapsed the
    /// redundant second "verify proximity" screen, since the ultrasonic
    /// approve flow that would justify it is deferred).
    ConfirmingDevice {
        device_name: String,
        code: String,
        // Dormant until the deferred ultrasonic-approve flow lands and a
        // proximity step re-consumes it to sign; the handshake already
        // plumbs it here (ADR-031 hardware events). Kept rather than
        // torn out of the EngineUpdate/bridge boundary for one release.
        #[allow(dead_code)]
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
pub const RETRY_ACTION_ID: &str = "retry";

/// Engine that drives the device linking workflow.
#[derive(Clone, Debug)]
pub struct DeviceLinkingEngine {
    step: DeviceLinkStep,
    qr_data: String,
    verification_code: Option<String>,
    locale: Locale,
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
            locale: Locale::English,
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
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S6b-6a).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
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
            DeviceLinkStep::VerifyCode | DeviceLinkStep::ConfirmingDevice { .. } => 2,
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
            DeviceLinkStep::Completing => self.completing_screen(),
            DeviceLinkStep::LinkFailed { message } => self.link_failed_screen(message),
        }
    }

    fn transport_selection_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_transport".into(),
            title: self.t("devices.link_new"),
            subtitle: Some(self.t("devices.link.how_would_you_like")),
            components: vec![Component::InfoPanel {
                id: "link_transport_info".into(),
                icon: Some("link".into()),
                title: self.t("devices.link.choose_connect"),
                items: vec![
                    InfoItem {
                        icon: Some("wifi".into()),
                        title: self.t("devices.link.via_internet"),
                        detail: self.t("devices.link.uses_relay"),
                    },
                    InfoItem {
                        icon: Some("qrcode".into()),
                        title: self.t("devices.link.offline_multipart"),
                        detail: self.t("devices.link.coming_soon_stub"),
                    },
                ],
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.transport_a11y_label")),
                    hint: Some(self.t("devices.link.transport_a11y_hint")),
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: TRANSPORT_INTERNET_ACTION_ID.into(),
                    label: self.t("devices.link.via_internet"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("devices.link.via_internet"))),
                },
                ScreenAction {
                    id: TRANSPORT_OFFLINE_ACTION_ID.into(),
                    label: self.t("devices.link.offline_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("devices.link.offline_button"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn offline_stub_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_offline_stub".into(),
            title: self.t("devices.link.offline_title"),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "offline_stub".into(),
                icon: Some("info".into()),
                title: self.t("devices.link.offline_not_available"),
                items: vec![InfoItem {
                    icon: None,
                    title: self.t("devices.link.use_internet_for_now"),
                    detail: self.t("devices.link.offline_future_release"),
                }],
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: BACK_TO_TRANSPORT_ACTION_ID.into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn show_qr_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_show_qr".into(),
            title: self.t("devices.link_device"),
            subtitle: None,
            components: vec![
                Component::QrCode {
                    id: "qr".into(),
                    data: self.qr_data.clone(),
                    mode: QrMode::Display,
                    label: Some(self.t("devices.link.scan_on_new_device")),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some(self.t("device_link.a11y_qr")),
                        hint: Some(self.t("devices.link.scan_to_begin_hint")),
                        role: Some(AccessibilityRole::Image),
                    }),
                },
                // M5 B2: the CLI-syntax join hint ("use: vauchi device
                // join <qr_data>") was shown on every frontend, phones
                // included (2026-07-03-second-device-join-dead-end item 2).
                // Removed — the scan-to-begin a11y hint above is the
                // GUI-appropriate guidance.
            ],
            actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn verify_code_screen(&self) -> ScreenModel {
        let code = self.verification_code.as_deref().unwrap_or("------");
        ScreenModel {
            screen_id: "link_verify".into(),
            title: self.t("devices.link.verify_device_title"),
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
                    title: self.t("devices.link.verify_this_code"),
                    items: vec![InfoItem {
                        icon: None,
                        title: self.t("devices.link.compare_codes"),
                        detail: self.t("devices.link.ensure_same_code"),
                    }],
                    a11y: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: CONFIRM_ACTION_ID.into(),
                    label: self.t("platform.button_confirm"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("platform.button_confirm"))),
                },
                ScreenAction {
                    id: REJECT_ACTION_ID.into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn syncing_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_syncing".into(),
            title: self.t("devices.link.syncing_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "syncing".into(),
                icon: None,
                title: self.t("devices.link.syncing_data"),
                detail: None,
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.syncing_data_a11y")),
                    hint: Some(self.t("devices.link.syncing_data_hint")),
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
            title: self.t("devices.link.device_linked_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "complete".into(),
                icon: None,
                title: self.t("devices.link.device_linked_title"),
                detail: None,
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.device_linked_status_a11y")),
                    hint: Some(self.t("devices.link.linked_success_hint")),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: DONE_ACTION_ID.into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn qr_pending_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_qr_pending".into(),
            title: self.t("devices.link_device"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "qr_pending".into(),
                icon: None,
                title: self.t("devices.generating_link"),
                detail: None,
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.generating_device_link")),
                    hint: Some(self.t("devices.link.preparing_qr_hint")),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn waiting_for_request_screen(&self, expires_at: &u64) -> ScreenModel {
        ScreenModel {
            screen_id: "link_waiting".into(),
            title: self.t("devices.link_device"),
            subtitle: None,
            components: vec![
                Component::QrCode {
                    id: "qr".into(),
                    data: self.qr_data.clone(),
                    mode: QrMode::Display,
                    label: Some(self.t("devices.link.scan_on_new_device")),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some(self.t("device_link.a11y_qr")),
                        hint: Some(self.t("devices.link.scan_to_begin_hint")),
                        role: Some(AccessibilityRole::Image),
                    }),
                },
                Component::Text {
                    id: "expires_at".into(),
                    content: get_string_with_args(
                        self.locale,
                        "devices.link.expires_at",
                        &[("expires_at", &expires_at.to_string())],
                    ),
                    style: TextStyle::Caption,
                },
            ],
            actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn qr_expired_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_qr_expired".into(),
            title: self.t("devices.link.qr_expired_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "qr_expired".into(),
                icon: Some("clock".into()),
                title: self.t("devices.qr_expired"),
                detail: Some(self.t("devices.link.qr_expired_detail")),
                status: Status::Warning,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.qr_expired_a11y")),
                    hint: Some(self.t("devices.link.qr_expired_hint")),
                    role: None,
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: self.t("devices.link.generate_new_qr"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("devices.link.generate_new_qr"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn confirming_device_screen(&self, device_name: &str, code: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_confirming_device".into(),
            title: self.t("devices.link.wants_to_link_title"),
            subtitle: Some(get_string_with_args(
                self.locale,
                "devices.link.device_subtitle",
                &[("device_name", device_name)],
            )),
            components: vec![
                Component::Text {
                    id: "code".into(),
                    content: code.to_string(),
                    style: TextStyle::Title,
                },
                Component::InfoPanel {
                    id: "confirm_device_info".into(),
                    icon: Some("shield".into()),
                    title: self.t("devices.link.verify_matches_new_device"),
                    items: vec![InfoItem {
                        icon: None,
                        title: self.t("devices.link.compare_codes"),
                        detail: self.t("devices.link.both_devices_same_code"),
                    }],
                    a11y: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: CODES_MATCH_ACTION_ID.into(),
                    label: self.t("devices.link.codes_match_verify"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("devices.link.codes_match_verify"))),
                },
                ScreenAction {
                    id: DENY_ACTION_ID.into(),
                    label: self.t("devices.link.deny"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("devices.link.deny"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn completing_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_completing".into(),
            title: self.t("devices.link.completing_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "completing".into(),
                icon: None,
                title: self.t("devices.link.sending_credentials"),
                detail: Some(self.t("devices.link.transferring_identity")),
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.completing_a11y")),
                    hint: Some(self.t("devices.link.sending_credentials_hint")),
                    role: None,
                }),
            }],
            actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn link_failed_screen(&self, message: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "link_failed".into(),
            title: self.t("devices.link.linking_failed_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "link_failed".into(),
                icon: Some("exclamationmark.triangle".into()),
                title: self.t("devices.link.linking_failed_status"),
                // M5 B2: map the machine's stable failure id to an honest
                // sentence — never render a raw "user_confirm_timeout" /
                // "user_denied" (2026-07-03-second-device-join-dead-end item 4).
                detail: Some(failure_detail(message, self.locale)),
                status: Status::Failed,
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.linking_failed_a11y")),
                    hint: Some(self.t("devices.link.could_not_complete")),
                    role: None,
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: RETRY_ACTION_ID.into(),
                    label: self.t("action.try_again"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.try_again"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }
}

/// Map the device-link machine's stable failure id (the
/// `DeviceLinkSessionListener::on_failed` reason) to a user-facing
/// sentence, so a raw machine id never reaches the screen (M5 B2,
/// mirrors `link_responder::failure_detail`).
fn failure_detail(reason: &str, locale: Locale) -> String {
    let key = match reason {
        "user_denied" => "device_link.failure_user_denied",
        "user_confirm_timeout" => "device_link.failure_confirm_timeout",
        "cancelled" => "device_link.failure_cancelled",
        "qr_expired" => "device_link.failure_qr_expired",
        _ => "device_link.failure_generic",
    };
    get_string(locale, key)
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
            // M5 B2b: "codes match" is the single confirmation. It moves
            // straight to the ephemeral Completing state and emits the
            // typed result so the app engine can call
            // `MobileDeviceLinkSession::confirm_manual(code, now)` — the
            // redundant second "verify proximity" screen was collapsed
            // (2026-07-03-second-device-join-dead-end item 5; the
            // ultrasonic-approve flow that would justify a distinct
            // proximity step is deferred).
            (DeviceLinkStep::ConfirmingDevice { code, .. }, CODES_MATCH_ACTION_ID) => {
                let code = code.clone();
                self.step = DeviceLinkStep::Completing;
                ActionResult::DeviceLinkConfirmManual { code }
            }
            // `deny` from receiver-side ConfirmingDevice. The app
            // engine intercepts `DeviceLinkDeny` to call
            // `MobileDeviceLinkSession::deny`; the cycle thread
            // emits `on_failed("user_denied")` + `on_session_ended()`
            // which collapses the sheet.
            (DeviceLinkStep::ConfirmingDevice { .. }, DENY_ACTION_ID) => {
                ActionResult::DeviceLinkDeny
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
