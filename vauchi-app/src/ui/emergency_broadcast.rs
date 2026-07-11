// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Emergency broadcast engine — configure trusted contacts and send a
//! disguised "I may be in danger" alert.
//!
//! Parsing of the comma-separated contact-id list and the trusted-contact
//! count limit live here (core), not in any frontend (ADR-021 / ADR-043).
//! The engine never touches storage: on `Complete` the AppEngine reads
//! [`EmergencyBroadcastEngine::outcome`] and performs the matching
//! `Vauchi` call (configure / send / delete).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::types::EmergencyBroadcastConfig;
use vauchi_core::{DEFAULT_EMERGENCY_MESSAGE, MAX_TRUSTED_CONTACTS};

/// What the AppEngine should do when the engine returns `Complete`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmergencyOutcome {
    /// Persist the configured contacts / message / location.
    Save,
    /// Send a broadcast to the configured trusted contacts.
    Send,
    /// Delete the emergency configuration.
    Disable,
}

/// Engine that drives the emergency-broadcast configure + send workflow.
pub struct EmergencyBroadcastEngine {
    step: EmergencyStep,
    configured: bool,
    /// All of the user's contacts — the pool the recipient picker renders.
    available_contacts: Vec<Item>,
    /// Ids (subset of `available_contacts`) chosen as trusted recipients.
    selected_contact_ids: Vec<String>,
    message: String,
    include_location: bool,
    pending_disable: bool,
    outcome: Option<EmergencyOutcome>,
    locale: Locale,
}

#[derive(Clone, Debug, PartialEq)]
enum EmergencyStep {
    Overview,
    ContactIds,
    Message,
    ConfirmSend,
}

impl EmergencyBroadcastEngine {
    /// Build from the stored config (`Some` = already configured).
    pub fn new(config: Option<EmergencyBroadcastConfig>) -> Self {
        match config {
            Some(c) => Self {
                step: EmergencyStep::Overview,
                configured: true,
                available_contacts: Vec::new(),
                selected_contact_ids: c.trusted_contact_ids,
                message: c.message,
                include_location: c.include_location,
                pending_disable: false,
                outcome: None,
                locale: Locale::English,
            },
            None => Self {
                step: EmergencyStep::Overview,
                configured: false,
                available_contacts: Vec::new(),
                selected_contact_ids: Vec::new(),
                message: String::new(),
                include_location: false,
                pending_disable: false,
                outcome: None,
                locale: Locale::English,
            },
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-6).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Inject the contact pool the picker renders. `screens.rs` supplies the
    /// full contact list; the stored selection (from config) is preserved.
    pub fn with_available_contacts(mut self, contacts: Vec<Item>) -> Self {
        self.available_contacts = contacts;
        self
    }

    /// The chosen trusted contact IDs (the picker's selected subset).
    pub fn contact_ids(&self) -> Vec<String> {
        self.selected_contact_ids.clone()
    }

    /// The configured alert message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the alert should include device location.
    pub fn include_location(&self) -> bool {
        self.include_location
    }

    /// What the AppEngine should do on `Complete` (set only on save / send /
    /// confirmed-disable).
    pub fn outcome(&self) -> Option<&EmergencyOutcome> {
        self.outcome.as_ref()
    }

    fn overview_screen(&self) -> ScreenModel {
        let broadcast_status = if self.configured {
            Status::Success
        } else {
            Status::Warning
        };
        let mut components = vec![
            Component::InfoPanel {
                id: "emergency_info".into(),
                icon: Some("warning".into()),
                title: self.t("emergency.title"),
                items: vec![InfoItem {
                    icon: Some("info".into()),
                    title: self.t("emergency.what_is_title"),
                    detail: self.t("emergency.what_is_detail"),
                }],
                a11y: None,
            },
            Component::StatusIndicator {
                id: "emergency_status".into(),
                icon: Some(
                    if self.configured {
                        "checkmark.circle.fill"
                    } else {
                        "exclamationmark.circle"
                    }
                    .into(),
                ),
                title: if self.configured {
                    self.t("emergency.configured_status")
                } else {
                    self.t("emergency.not_set_up_status")
                },
                detail: None,
                status: broadcast_status,
                status_label: self.t(broadcast_status.label_key()),
                a11y: None,
            },
        ];

        let configure_label = if self.configured {
            self.t("emergency.edit_contacts_button")
        } else {
            self.t("emergency.configure_button")
        };
        let mut actions = vec![ScreenAction {
            id: "configure".into(),
            label: configure_label.clone(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: Some(A11y::labeled(configure_label)),
        }];

        if self.configured {
            actions.push(ScreenAction {
                id: "send".into(),
                label: self.t("emergency.send_alert_button"),
                style: ActionStyle::Destructive,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("emergency.send_alert_button"))),
            });
            actions.push(ScreenAction {
                id: "disable".into(),
                label: self.t("resistance.duress.disable_button"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("resistance.duress.disable_button"))),
            });
        }

        if self.pending_disable {
            components.push(Component::InlineConfirm {
                id: "disable".into(),
                warning: self.t("emergency.disable_warning"),
                confirm_text: self.t("resistance.duress.disable_button"),
                cancel_text: self.t("action.cancel"),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(self.t("emergency.confirm_disable_a11y")),
                    hint: None,
                    role: Some(AccessibilityRole::Alert),
                }),
            });
        }

        ScreenModel {
            screen_id: "emergency_overview".into(),
            title: self.t("emergency.title"),
            subtitle: None,
            components,
            actions,
            progress: None,
            ..Default::default()
        }
    }

    fn contact_ids_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "emergency_contacts".into(),
            title: self.t("emergency.contact_ids_title"),
            subtitle: None,
            components: vec![Component::ToggleList {
                id: "contact_ids".into(),
                label: self.t("emergency.contact_ids_title"),
                items: self
                    .available_contacts
                    .iter()
                    .map(|c| ToggleItem {
                        id: c.id.clone(),
                        label: c.name.clone(),
                        selected: self.selected_contact_ids.contains(&c.id),
                        subtitle: None,
                        a11y: Some(A11y::labeled(c.name.clone())),
                        info_key: None,
                    })
                    .collect(),
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
                ScreenAction {
                    id: "continue".into(),
                    label: self.t("action.continue"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.continue"))),
                },
            ],
            progress: Some(Progress {
                current_step: 1,
                total_steps: 2,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn message_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "emergency_message".into(),
            title: self.t("emergency.message_label"),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "message".into(),
                    label: self.t("emergency.alert_message_input_label"),
                    value: self.message.clone(),
                    placeholder: Some(self.t("emergency.alert_message_placeholder")),
                    max_length: None,
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("emergency.alert_message_input_a11y")),
                        hint: None,
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
                Component::ToggleList {
                    id: "options".into(),
                    label: self.t("emergency.options_label"),
                    items: vec![ToggleItem {
                        id: "include_location".into(),
                        label: self.t("emergency.include_location"),
                        selected: self.include_location,
                        subtitle: Some(self.t("emergency.include_location_desc")),
                        a11y: Some(A11y::labeled(self.t("emergency.include_location"))),
                        info_key: None,
                    }],
                    a11y: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
                ScreenAction {
                    id: "save".into(),
                    label: self.t("action.save"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
            ],
            progress: Some(Progress {
                current_step: 2,
                total_steps: 2,
                label: None,
            }),
            ..Default::default()
        }
    }

    fn confirm_send_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "emergency_confirm_send".into(),
            title: self.t("emergency.confirm_send_title"),
            subtitle: None,
            components: vec![Component::InlineConfirm {
                id: "send".into(),
                warning: self.t("emergency.confirm_send_warning"),
                confirm_text: self.t("emergency.send_alert_button"),
                cancel_text: self.t("action.cancel"),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(self.t("emergency.confirm_send_a11y")),
                    hint: None,
                    role: Some(AccessibilityRole::Alert),
                }),
            }],
            actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for EmergencyBroadcastEngine {
    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::EmergencyBroadcast(EmergencyBroadcastPlan {
            outcome: self.outcome().cloned(),
            contact_ids: self.contact_ids(),
            message: self.message().to_string(),
            include_location: self.include_location(),
        }))
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            EmergencyStep::Overview => self.overview_screen(),
            EmergencyStep::ContactIds => self.contact_ids_screen(),
            EmergencyStep::Message => self.message_screen(),
            EmergencyStep::ConfirmSend => self.confirm_send_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // --- Overview ---
            (EmergencyStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "configure" =>
            {
                if self.message.trim().is_empty() {
                    self.message = DEFAULT_EMERGENCY_MESSAGE.to_string();
                }
                self.step = EmergencyStep::ContactIds;
                ActionResult::NavigateTo(self.current_screen())
            }
            (EmergencyStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "send" && self.configured =>
            {
                self.step = EmergencyStep::ConfirmSend;
                ActionResult::NavigateTo(self.current_screen())
            }
            (EmergencyStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "disable" && self.configured =>
            {
                self.pending_disable = true;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (EmergencyStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "confirm_disable" =>
            {
                self.pending_disable = false;
                self.outcome = Some(EmergencyOutcome::Disable);
                ActionResult::Complete
            }
            (EmergencyStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "cancel_disable" =>
            {
                self.pending_disable = false;
                ActionResult::UpdateScreen(self.current_screen())
            }

            // --- ContactIds ---
            (
                EmergencyStep::ContactIds,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "contact_ids" => {
                if let Some(pos) = self
                    .selected_contact_ids
                    .iter()
                    .position(|id| id == &item_id)
                {
                    self.selected_contact_ids.remove(pos);
                } else {
                    self.selected_contact_ids.push(item_id);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            (EmergencyStep::ContactIds, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                let ids = self.contact_ids();
                if ids.is_empty() {
                    ActionResult::ValidationError {
                        component_id: "contact_ids".into(),
                        message: self.t("emergency.min_one_contact_error"),
                    }
                } else if ids.len() > MAX_TRUSTED_CONTACTS {
                    ActionResult::ValidationError {
                        component_id: "contact_ids".into(),
                        message: get_string_with_args(
                            self.locale,
                            "emergency.max_contacts_error",
                            &[("max", &MAX_TRUSTED_CONTACTS.to_string())],
                        ),
                    }
                } else {
                    self.step = EmergencyStep::Message;
                    ActionResult::NavigateTo(self.current_screen())
                }
            }
            (EmergencyStep::ContactIds, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = EmergencyStep::Overview;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- Message ---
            (
                EmergencyStep::Message,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "message" => {
                self.message = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                EmergencyStep::Message,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "options" && item_id == "include_location" => {
                self.include_location = !self.include_location;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (EmergencyStep::Message, UserAction::ActionPressed { action_id })
                if action_id == "save" || action_id == "submit_message" =>
            {
                self.configured = true;
                self.outcome = Some(EmergencyOutcome::Save);
                ActionResult::Complete
            }
            (EmergencyStep::Message, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = EmergencyStep::ContactIds;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- ConfirmSend ---
            (EmergencyStep::ConfirmSend, UserAction::ActionPressed { action_id })
                if action_id == "confirm_send" =>
            {
                self.outcome = Some(EmergencyOutcome::Send);
                ActionResult::Complete
            }
            (EmergencyStep::ConfirmSend, UserAction::ActionPressed { action_id })
                if action_id == "cancel_send" =>
            {
                self.step = EmergencyStep::Overview;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- Fallback ---
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
