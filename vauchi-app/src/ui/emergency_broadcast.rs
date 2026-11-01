// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Emergency broadcast engine (ADR-036, private#895) — one screen: what an
//! alert is, whether it is set up, the Send button, the trusted-contact
//! picker (at most ten), the alert message and the location choice.
//!
//! The engine edits the form and arms the two questions (send, disable).
//! The `AppEngine` intercept performs the Core calls on
//! `confirm_send_alert`, `save_emergency_config` and
//! `confirm_disable_emergency_broadcast`, then rebuilds this engine from
//! storage. Copy resolves through `i18n::get_string` in the `emergency.*`
//! family.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use vauchi_core::Command;
use vauchi_core::emergency::MAX_TRUSTED_CONTACTS;

pub const TRUSTED_CONTACTS_COMPONENT_ID: &str = "trusted_contacts";
pub const MESSAGE_COMPONENT_ID: &str = "message";
pub const OPTIONS_COMPONENT_ID: &str = "options";
pub const INCLUDE_LOCATION_ITEM_ID: &str = "include_location";
/// The two `ButtonList`s; a shell reports a tap on one of their buttons
/// as `ListItemSelected`, not `ActionPressed`.
pub const SEND_ACTIONS_COMPONENT_ID: &str = "send_actions";
pub const CONFIG_ACTIONS_COMPONENT_ID: &str = "config_actions";
pub const SEND_ALERT_ACTION_ID: &str = "send_alert";
pub const CONFIRM_SEND_ACTION_ID: &str = "confirm_send_alert";
pub const CANCEL_SEND_ACTION_ID: &str = "cancel_send_alert";
pub const SAVE_CONFIG_ACTION_ID: &str = "save_emergency_config";
pub const DISABLE_ACTION_ID: &str = "disable_emergency_broadcast";
pub const CONFIRM_DISABLE_ACTION_ID: &str = "confirm_disable_emergency_broadcast";
pub const CANCEL_DISABLE_ACTION_ID: &str = "cancel_disable_emergency_broadcast";

/// How long the "Send alert to N contacts?" question stays open before it
/// closes by itself (`emergency_broadcast :: Emergency broadcast with
/// confirmation`). A question left open on a phone in a pocket must not
/// fire on a stray touch minutes later.
pub const CONFIRM_WINDOW_SECS: u64 = 5;

/// Longest alert message the form accepts (`Configure alert message`).
const MESSAGE_MAX_CHARS: usize = 500;

/// One contact offered as an alert recipient.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TrustedContactChoice {
    pub id: String,
    pub display_name: String,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Armed {
    /// `asked_at` is the first tick seen after arming: the engine has no
    /// clock of its own, so the window is measured from the next poll.
    Send {
        asked_at: Option<u64>,
    },
    Disable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SendOutcome {
    sent: usize,
    total: usize,
}

/// Emergency broadcast configure-and-send engine.
#[derive(Clone, Debug)]
pub struct EmergencyBroadcastEngine {
    contacts: Vec<TrustedContactChoice>,
    message: String,
    include_location: bool,
    configured: bool,
    armed: Option<Armed>,
    contacts_error: Option<String>,
    last_send: Option<SendOutcome>,
    locale: Locale,
}

impl EmergencyBroadcastEngine {
    pub fn new(
        contacts: Vec<TrustedContactChoice>,
        message: String,
        include_location: bool,
        configured: bool,
        locale: Locale,
    ) -> Self {
        Self {
            contacts,
            message,
            include_location,
            configured,
            armed: None,
            contacts_error: None,
            last_send: None,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn selected_ids(&self) -> Vec<String> {
        self.contacts
            .iter()
            .filter(|c| c.selected)
            .map(|c| c.id.clone())
            .collect()
    }

    fn selected_count(&self) -> usize {
        self.contacts.iter().filter(|c| c.selected).count()
    }

    fn what_is_panel(&self) -> Component {
        Component::InfoPanel {
            id: "what_is".into(),
            icon: Some("warning".into()),
            title: self.t("emergency.what_is_title"),
            items: vec![InfoItem {
                icon: Some("shield".into()),
                title: self.t("emergency.description"),
                detail: self.t("emergency.what_is_detail"),
            }],
            a11y: None,
        }
    }

    fn status_indicator(&self) -> Component {
        let (status, title) = if self.configured {
            (Status::Success, self.t("emergency.configured_status"))
        } else {
            (Status::Pending, self.t("emergency.not_set_up_status"))
        };
        let (status, detail) = match self.last_send {
            Some(SendOutcome { sent, total }) if sent == total => (
                Status::Success,
                Some(get_string_with_args(
                    self.locale,
                    "emergency.broadcast_sent",
                    &[("count", &sent.to_string())],
                )),
            ),
            Some(SendOutcome { sent, total }) => (
                Status::Warning,
                Some(get_string_with_args(
                    self.locale,
                    "emergency.sent_partial",
                    &[("sent", &sent.to_string()), ("total", &total.to_string())],
                )),
            ),
            None => (status, None),
        };
        Component::StatusIndicator {
            id: "status".into(),
            icon: None,
            title,
            detail,
            status,
            status_label: self.t(status.label_key()),
            a11y: None,
        }
    }

    fn send_block(&self) -> Option<Component> {
        if !self.configured {
            return None;
        }
        Some(match self.armed {
            Some(Armed::Send { .. }) => Component::InlineConfirm {
                id: "send".into(),
                warning: format!(
                    "{} {}",
                    get_string_with_args(
                        self.locale,
                        "emergency.confirm_send_count",
                        &[("count", &self.selected_count().to_string())],
                    ),
                    self.t("emergency.confirm_send_warning")
                ),
                confirm_text: self.t("emergency.send_alert_button"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: CONFIRM_SEND_ACTION_ID.into(),
                cancel_action_id: CANCEL_SEND_ACTION_ID.into(),
                destructive: true,
                a11y: Some(A11y::labeled(self.t("emergency.confirm_send_a11y"))),
            },
            _ => Component::ButtonList {
                id: SEND_ACTIONS_COMPONENT_ID.into(),
                items: vec![ActionListItem {
                    id: SEND_ALERT_ACTION_ID.into(),
                    label: self.t("emergency.send_alert_button"),
                    icon: Some("warning".into()),
                    detail: None,
                    a11y: Some(A11y::labeled(self.t("emergency.send"))),
                    info_key: None,
                }],
            },
        })
    }

    fn contacts_block(&self) -> Vec<Component> {
        let mut components = Vec::new();
        if self.contacts.is_empty() {
            components.push(Component::Text {
                id: "no_contacts".into(),
                content: self.t("emergency.no_contacts_hint"),
                style: TextStyle::Body,
                a11y: None,
            });
            return components;
        }
        components.push(Component::ToggleList {
            id: TRUSTED_CONTACTS_COMPONENT_ID.into(),
            label: self.t("emergency.trusted_contacts"),
            items: self
                .contacts
                .iter()
                .map(|c| ToggleItem {
                    id: c.id.clone(),
                    label: c.display_name.clone(),
                    selected: c.selected,
                    subtitle: None,
                    a11y: None,
                    info_key: None,
                })
                .collect(),
            a11y: Some(A11y {
                label: Some(self.t("emergency.trusted_contacts")),
                hint: Some(get_string_with_args(
                    self.locale,
                    "emergency.max_contacts_error",
                    &[("max", &MAX_TRUSTED_CONTACTS.to_string())],
                )),
                role: None,
            }),
        });
        if let Some(error) = &self.contacts_error {
            components.push(Component::Text {
                id: "contacts_error".into(),
                content: error.clone(),
                style: TextStyle::Caption,
                a11y: None,
            });
        }
        components
    }

    fn message_input(&self) -> Component {
        Component::TextInput {
            id: MESSAGE_COMPONENT_ID.into(),
            label: self.t("emergency.message_label"),
            value: self.message.clone(),
            placeholder: Some(self.t("emergency.alert_message_placeholder")),
            max_length: Some(MESSAGE_MAX_CHARS),
            validation_error: None,
            input_type: InputType::Text,
            a11y: Some(A11y::labeled(self.t("emergency.alert_message_input_a11y"))),
            info_key: None,
        }
    }

    fn options_list(&self) -> Component {
        Component::ToggleList {
            id: OPTIONS_COMPONENT_ID.into(),
            label: self.t("emergency.options_label"),
            items: vec![ToggleItem {
                id: INCLUDE_LOCATION_ITEM_ID.into(),
                label: self.t("emergency.include_location"),
                selected: self.include_location,
                subtitle: Some(self.t("emergency.include_location_desc")),
                a11y: None,
                info_key: None,
            }],
            a11y: None,
        }
    }

    fn config_block(&self) -> Component {
        if self.armed == Some(Armed::Disable) {
            return Component::InlineConfirm {
                id: "disable".into(),
                warning: self.t("emergency.disable_warning"),
                confirm_text: self.t("resistance.emergency.disable"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: CONFIRM_DISABLE_ACTION_ID.into(),
                cancel_action_id: CANCEL_DISABLE_ACTION_ID.into(),
                destructive: true,
                a11y: Some(A11y::labeled(self.t("emergency.confirm_disable_a11y"))),
            };
        }
        let mut items = vec![ActionListItem {
            id: SAVE_CONFIG_ACTION_ID.into(),
            label: self.t("action.save"),
            icon: Some("checkmark".into()),
            detail: None,
            a11y: None,
            info_key: None,
        }];
        if self.configured {
            items.push(ActionListItem {
                id: DISABLE_ACTION_ID.into(),
                label: self.t("resistance.emergency.disable"),
                icon: Some("delete".into()),
                detail: None,
                a11y: None,
                info_key: None,
            });
        }
        Component::ButtonList {
            id: CONFIG_ACTIONS_COMPONENT_ID.into(),
            items,
        }
    }

    fn toggle_contact(&mut self, item_id: &str) -> ActionResult {
        let at_limit = self.selected_count() >= MAX_TRUSTED_CONTACTS;
        let Some(contact) = self.contacts.iter_mut().find(|c| c.id == item_id) else {
            return ActionResult::UpdateScreen(self.current_screen());
        };
        if !contact.selected && at_limit {
            self.contacts_error = Some(get_string_with_args(
                self.locale,
                "emergency.max_contacts_error",
                &[("max", &MAX_TRUSTED_CONTACTS.to_string())],
            ));
        } else {
            contact.selected = !contact.selected;
            self.contacts_error = None;
        }
        ActionResult::UpdateScreen(self.current_screen())
    }

    /// A confirm or cancel on one of the two questions (`ActionPressed`).
    fn press(&mut self, action_id: &str) -> ActionResult {
        if matches!(
            action_id,
            CANCEL_SEND_ACTION_ID | CONFIRM_SEND_ACTION_ID | CANCEL_DISABLE_ACTION_ID
        ) {
            self.armed = None;
        }
        ActionResult::UpdateScreen(self.current_screen())
    }

    /// A button in one of the two `ButtonList`s (`ListItemSelected`).
    fn pick(&mut self, item_id: &str) -> ActionResult {
        match item_id {
            SEND_ALERT_ACTION_ID if self.configured => {
                self.armed = Some(Armed::Send { asked_at: None });
            }
            DISABLE_ACTION_ID if self.configured => self.armed = Some(Armed::Disable),
            SAVE_CONFIG_ACTION_ID if self.selected_count() == 0 => {
                self.contacts_error = Some(self.t("emergency.min_one_contact_error"));
            }
            _ => {}
        }
        ActionResult::UpdateScreen(self.current_screen())
    }
}

impl WorkflowEngine for EmergencyBroadcastEngine {
    fn current_screen(&self) -> ScreenModel {
        let mut components = vec![self.what_is_panel(), self.status_indicator()];
        components.extend(self.send_block());
        components.extend(self.contacts_block());
        components.push(self.message_input());
        components.push(self.options_list());
        components.push(self.config_block());
        ScreenModel {
            screen_id: "emergency_broadcast".into(),
            title: self.t("emergency.title"),
            subtitle: Some(self.t("emergency.description")),
            components,
            contextual_actions: vec![],
            ..Default::default()
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == TRUSTED_CONTACTS_COMPONENT_ID => self.toggle_contact(&item_id),
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == OPTIONS_COMPONENT_ID && item_id == INCLUDE_LOCATION_ITEM_ID => {
                self.include_location = !self.include_location;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == MESSAGE_COMPONENT_ID => {
                self.message = value.chars().take(MESSAGE_MAX_CHARS).collect();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } => self.press(&action_id),
            UserAction::ListItemSelected {
                component_id,
                item_id,
            } if component_id == SEND_ACTIONS_COMPONENT_ID
                || component_id == CONFIG_ACTIONS_COMPONENT_ID =>
            {
                self.pick(&item_id)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::EmergencyBroadcast {
            trusted_contact_ids: self.selected_ids(),
            message: self.message.clone(),
            include_location: self.include_location,
        })
    }

    fn apply_update(&mut self, update: EngineUpdate) -> bool {
        match update {
            EngineUpdate::EmergencyBroadcastSent { sent, total } => {
                self.last_send = Some(SendOutcome { sent, total });
                self.armed = None;
                true
            }
            _ => false,
        }
    }

    fn tick(&mut self, now: u64) -> Vec<Command> {
        if let Some(Armed::Send { asked_at }) = self.armed {
            match asked_at {
                None => {
                    self.armed = Some(Armed::Send {
                        asked_at: Some(now),
                    })
                }
                Some(since) if now >= since.saturating_add(CONFIRM_WINDOW_SECS) => {
                    self.armed = None;
                }
                Some(_) => {}
            }
        }
        Vec::new()
    }
}
