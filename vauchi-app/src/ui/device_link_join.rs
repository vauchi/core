// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device-link **join** (responder) engine — M5 B3 Slice 3.
//!
//! Humble display engine for the fresh device that receives a
//! [`DeviceLinkJoinInvitation`]. Core owns the responder machine and
//! lifecycle; this engine only renders the steps and forwards user
//! actions.
//!
//! Flow:
//! ```text
//! EnterName → PostingRequest → AwaitingResponse { confirmation_code }
//!     → Completing → Complete
//!     └─ Failed (retry returns to EnterName)
//! ```
//! The user confirms the device name, then verifies the confirmation code
//! shown on the initiator device. That code is the human acknowledgment
//! gate; the initiator releases the master seed only after its user taps
//! "codes match".

use crate::i18n::{Locale, get_string};
use crate::ui::*;

/// Action ids handled by `DeviceLinkJoinEngine`.
pub const JOIN_ACTION_ID: &str = "join";
pub const RETRY_ACTION_ID: &str = "retry";
pub const CANCEL_ACTION_ID: &str = "cancel";

/// Component id for the device name text field.
pub const DEVICE_NAME_INPUT_ID: &str = "device_name";

/// Steps in the device-link join flow.
#[derive(Clone, Debug, PartialEq)]
enum JoinStep {
    /// User confirms/edits the name for the new device before posting
    /// the join request.
    EnterName,
    /// Request is being posted to the relay.
    PostingRequest,
    /// Request posted; waiting for the initiator to confirm and respond.
    /// `confirmation_code` must match the code shown on the initiator.
    AwaitingResponse { confirmation_code: String },
    /// Response received; adopting the identity.
    Completing,
    /// Identity adopted.
    Complete,
    /// Terminal failure; `reason` is a stable machine id mapped to honest
    /// user copy via `failure_detail`.
    Failed { reason: String },
}

/// Humble display engine for joining an existing identity from a fresh
/// device.
#[derive(Clone, Debug)]
pub struct DeviceLinkJoinEngine {
    step: JoinStep,
    /// What the user typed. Empty until they do — the default is offered
    /// as a placeholder rather than as content, because a caret lands
    /// after existing text and typing would otherwise concatenate the
    /// two.
    device_name: String,
    /// The device's own name, applied when the field is left untouched.
    default_device_name: String,
    locale: Locale,
}

impl DeviceLinkJoinEngine {
    /// Build a join engine starting at the name-entry step.
    pub fn new(default_device_name: String) -> Self {
        Self {
            step: JoinStep::EnterName,
            device_name: String::new(),
            default_device_name,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    /// The name to register this device under: what the user typed, or
    /// the device's own name when the field was left untouched.
    pub fn device_name(&self) -> &str {
        if self.device_name.trim().is_empty() {
            &self.default_device_name
        } else {
            &self.device_name
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            JoinStep::EnterName => self.enter_name_screen(),
            JoinStep::PostingRequest => self.posting_request_screen(),
            JoinStep::AwaitingResponse { confirmation_code } => {
                self.awaiting_response_screen(confirmation_code)
            }
            JoinStep::Completing => self.completing_screen(),
            JoinStep::Complete => self.complete_screen(),
            JoinStep::Failed { reason } => self.failed_screen(reason),
        }
    }

    fn enter_name_screen(&self) -> ScreenModel {
        // Gated on the effective name, so an untouched field still joins
        // under the device's own name.
        let can_join = !self.device_name().trim().is_empty();
        ScreenModel {
            screen_id: "device_link_join".into(),
            title: self.t("devices.link.join_title"),
            subtitle: Some(self.t("devices.link.paste_data")),
            components: vec![Component::TextInput {
                id: DEVICE_NAME_INPUT_ID.into(),
                label: self.t("devices.link.device_name"),
                value: self.device_name.clone(),
                placeholder: Some(self.default_device_name.clone()),
                max_length: None,
                validation_error: None,
                input_type: InputType::Text,
                a11y: Some(A11y::labeled(self.t("devices.link.device_name"))),
                info_key: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: JOIN_ACTION_ID.into(),
                    label: self.t("action.confirm"),
                    style: ActionStyle::Primary,
                    enabled: can_join,
                    a11y: Some(A11y::labeled(self.t("action.confirm"))),
                },
                ScreenAction {
                    id: CANCEL_ACTION_ID.into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn posting_request_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_join_posting".into(),
            title: self.t("devices.link.join_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "posting_request".into(),
                icon: None,
                title: self.t("devices.link.waiting_approval"),
                detail: None,
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.waiting_approval")),
                    hint: None,
                    role: None,
                }),
            }],
            contextual_actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn awaiting_response_screen(&self, confirmation_code: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_join_confirm".into(),
            title: self.t("devices.link.join_title"),
            subtitle: Some(self.t("devices.link.confirm_code_match")),
            components: vec![
                Component::Text {
                    a11y: None,
                    id: "confirmation_code".into(),
                    content: confirmation_code.to_string(),
                    style: TextStyle::Title,
                },
                Component::InfoPanel {
                    id: "confirm_info".into(),
                    icon: Some("shield".into()),
                    title: self.t("devices.link.verify_matches_new_device"),
                    items: vec![InfoItem {
                        icon: None,
                        title: self.t("devices.link.compare_codes"),
                        detail: self.t("devices.link.ensure_same_code"),
                    }],
                    a11y: None,
                },
            ],
            contextual_actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.cancel"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.cancel"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn completing_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_join_completing".into(),
            title: self.t("devices.link.completing_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "completing".into(),
                icon: None,
                title: self.t("devices.link.sending_credentials"),
                detail: Some(self.t("devices.link.transferring_identity")),
                status: Status::InProgress,
                status_label: self.t(Status::InProgress.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.completing_a11y")),
                    hint: Some(self.t("devices.link.sending_credentials_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn complete_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_join_complete".into(),
            title: self.t("devices.link.device_linked_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "complete".into(),
                icon: None,
                title: self.t("devices.link.join_success"),
                detail: None,
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.device_linked_status_a11y")),
                    hint: Some(self.t("devices.link.linked_success_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![ScreenAction {
                id: CANCEL_ACTION_ID.into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn failed_screen(&self, reason: &str) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_join_failed".into(),
            title: self.t("devices.link.linking_failed_title"),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "link_failed".into(),
                icon: Some("exclamationmark.triangle".into()),
                title: self.t("devices.link.linking_failed_status"),
                detail: Some(failure_detail(reason, self.locale)),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: Some(A11y {
                    label: Some(self.t("devices.link.linking_failed_a11y")),
                    hint: Some(self.t("devices.link.could_not_complete")),
                    role: None,
                }),
            }],
            contextual_actions: vec![
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
            progress: None,
            ..Default::default()
        }
    }
}

/// Map the responder machine's stable failure id to a user-facing
/// sentence, mirroring `device_linking::failure_detail`.
fn failure_detail(reason: &str, locale: Locale) -> String {
    let key = match reason {
        "qr_expired" => "device_link.failure_qr_expired",
        "relay_failed" => "device_link.failure_generic",
        "decrypt_error" => "device_link.failure_generic",
        "invalid_qr" => "device_link.failure_generic",
        _ => "device_link.failure_generic",
    };
    get_string(locale, key)
}

impl WorkflowEngine for DeviceLinkJoinEngine {
    fn apply_update(&mut self, update: EngineUpdate) -> bool {
        let EngineUpdate::DeviceLinkJoin(update) = update else {
            return false;
        };
        match update {
            DeviceLinkJoinUpdate::NameAccepted => self.step = JoinStep::PostingRequest,
            DeviceLinkJoinUpdate::RequestPosted { confirmation_code } => {
                self.step = JoinStep::AwaitingResponse { confirmation_code }
            }
            DeviceLinkJoinUpdate::ResponseReady => self.step = JoinStep::Completing,
            DeviceLinkJoinUpdate::Completed => self.step = JoinStep::Complete,
            DeviceLinkJoinUpdate::Failed(reason) => self.step = JoinStep::Failed { reason },
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == DEVICE_NAME_INPUT_ID => {
                self.device_name = value;
                ActionResult::UpdateScreen(self.build_screen())
            }
            UserAction::ActionPressed { action_id } => match action_id.as_str() {
                JOIN_ACTION_ID => {
                    // Effective name: an untouched field means "use this
                    // device's own name", so only a blank *default* is a
                    // validation failure.
                    if self.device_name().trim().is_empty() {
                        return ActionResult::ValidationError {
                            component_id: DEVICE_NAME_INPUT_ID.into(),
                            message: self.t("devices.link.device_name"),
                        };
                    }
                    let name = self.device_name().to_string();
                    self.step = JoinStep::PostingRequest;
                    ActionResult::DeviceLinkJoinStart { device_name: name }
                }
                RETRY_ACTION_ID => {
                    self.step = JoinStep::EnterName;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                CANCEL_ACTION_ID => ActionResult::Complete,
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: private JoinStep transitions and screen building.
#[cfg(test)]
#[path = "device_link_join_tests.rs"]
mod tests;
