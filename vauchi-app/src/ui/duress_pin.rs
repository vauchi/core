// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress PIN engine — configure a duress PIN that triggers silent alerts.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3c of `2026-07-03-core-screens-bypass-i18n`); keys
//! live in the `resistance.duress.*` + shared `action.*` families.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

/// Configuration for the duress PIN feature.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DuressConfig {
    pub enabled: bool,
    /// All of the user's contacts — the pool the recipient picker renders.
    pub available_contacts: Vec<Item>,
    /// Ids (subset of `available_contacts`) chosen to receive the duress alert.
    pub selected_contact_ids: Vec<String>,
    pub alert_message: String,
    pub include_location: bool,
}

/// Taken from core so the reducer and the API cannot disagree about how
/// long a PIN is. A local copy is how they drifted: this reducer capped
/// typed input at six while a pasted value bypassed the cap entirely and
/// `setup_duress_password` accepted whatever arrived.
use vauchi_core::emergency::DURESS_PIN_LENGTH as PIN_LENGTH;

/// Folds one input event into a PIN buffer.
///
/// A shell may deliver a single keystroke or a whole string — a paste, an
/// autofill, an accessibility insertion, or a harness typing the field at
/// once. Treating the multi-character case as "assign it verbatim" is what
/// let the literal string `undefined` become a duress credential on a real
/// device, so every shape arrives here and is filtered to ASCII digits and
/// truncated identically.
fn accept_pin_input(current: &str, value: &str) -> String {
    let mut out = String::from(current);
    for b in value.bytes().filter(|b| b.is_ascii_digit()) {
        if out.len() == PIN_LENGTH {
            break;
        }
        out.push(char::from(b));
    }
    out
}

/// Engine that drives the duress PIN setup workflow.
pub struct DuressPinEngine {
    step: DuressPinStep,
    config: DuressConfig,
    new_pin: String,
    confirm_pin: String,
    pending_disable: bool,
    locale: Locale,
}

impl Drop for DuressPinEngine {
    fn drop(&mut self) {
        self.new_pin.zeroize();
        self.confirm_pin.zeroize();
    }
}

#[derive(Clone, Debug, PartialEq)]
enum DuressPinStep {
    Overview,
    EnterPin,
    ConfirmPin,
    ConfigureAlerts,
}

impl DuressPinEngine {
    pub fn new(config: DuressConfig, locale: Locale) -> Self {
        Self {
            step: DuressPinStep::Overview,
            config,
            new_pin: String::new(),
            confirm_pin: String::new(),
            pending_disable: false,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    pub fn config(&self) -> &DuressConfig {
        &self.config
    }

    /// Returns the confirmed PIN (only valid after save completion).
    pub fn pin(&self) -> &str {
        &self.new_pin
    }

    fn progress(&self) -> Progress {
        let current_step = match self.step {
            DuressPinStep::Overview => 1,
            DuressPinStep::EnterPin => 2,
            DuressPinStep::ConfirmPin => 3,
            DuressPinStep::ConfigureAlerts => 4,
        };
        Progress {
            current_step,
            total_steps: 4,
            label: None,
        }
    }

    fn overview_screen(&self) -> ScreenModel {
        let duress_status = if self.config.enabled {
            Status::Success
        } else {
            Status::Warning
        };
        let mut components = vec![
            Component::InfoPanel {
                id: "duress_info".into(),
                icon: Some("shield".into()),
                title: self.t("resistance.duress.pin_label"),
                items: vec![InfoItem {
                    icon: Some("info".into()),
                    title: self.t("resistance.duress.what_is_title"),
                    detail: self.t("resistance.duress.what_is_detail"),
                }],
                a11y: None,
            },
            Component::StatusIndicator {
                id: "duress_status".into(),
                icon: Some(
                    if self.config.enabled {
                        "checkmark.shield.fill"
                    } else {
                        "exclamationmark.shield"
                    }
                    .into(),
                ),
                title: if self.config.enabled {
                    self.t("resistance.duress.enabled")
                } else {
                    self.t("resistance.duress.not_set_up")
                },
                detail: None,
                status: duress_status,
                status_label: self.t(duress_status.label_key()),
                a11y: None,
            },
        ];

        let configure_label = if self.config.enabled {
            self.t("resistance.duress.change_pin")
        } else {
            self.t("resistance.duress.set_up_pin")
        };
        let mut actions = vec![ScreenAction {
            id: "configure".into(),
            label: configure_label.clone(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: Some(A11y::labeled(configure_label)),
        }];

        if self.config.enabled {
            actions.push(ScreenAction {
                id: "disable".into(),
                label: self.t("resistance.duress.disable_button"),
                style: ActionStyle::Destructive,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("resistance.duress.disable_button"))),
            });
        }

        if self.pending_disable {
            components.push(Component::InlineConfirm {
                id: "disable".into(),
                warning: self.t("resistance.duress.disable_warning"),
                confirm_text: self.t("resistance.duress.disable_button"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_disable".into(),
                cancel_action_id: "cancel_disable".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some(self.t("resistance.duress.disable_a11y")),
                    hint: Some(self.t("resistance.duress.disable_a11y_hint")),
                    role: Some(AccessibilityRole::Alert),
                }),
            });
        }

        ScreenModel {
            screen_id: "duress_overview".into(),
            title: self.t("resistance.duress.pin_label"),
            subtitle: None,
            components,
            contextual_actions: actions,
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn enter_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_enter_pin".into(),
            title: self.t("resistance.duress.setup"),
            subtitle: None,
            components: vec![Component::PinInput {
                id: "pin".into(),
                label: self.t("resistance.duress.enter_pin"),
                length: PIN_LENGTH,
                filled: self.new_pin.len(),
                masked: true,
                validation_error: None,
                a11y: Some(A11y {
                    label: Some(self.t("resistance.duress.enter_pin")),
                    hint: Some(self.t("resistance.duress.enter_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![
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
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn confirm_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_confirm_pin".into(),
            title: self.t("resistance.duress.confirm_pin"),
            subtitle: None,
            components: vec![Component::PinInput {
                id: "confirm_pin".into(),
                label: self.t("resistance.duress.confirm_pin"),
                length: PIN_LENGTH,
                filled: self.confirm_pin.len(),
                masked: true,
                validation_error: None,
                a11y: Some(A11y {
                    label: Some(self.t("resistance.duress.confirm_pin")),
                    hint: Some(self.t("resistance.duress.confirm_hint")),
                    role: None,
                }),
            }],
            contextual_actions: vec![
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
            progress: Some(self.progress()),
            ..Default::default()
        }
    }

    fn alerts_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_alerts".into(),
            title: self.t("resistance.duress.configure_alerts"),
            subtitle: None,
            components: vec![
                Component::ToggleList {
                    id: "recipients".into(),
                    label: self.t("resistance.duress.setup.alert_recipients"),
                    items: self
                        .config
                        .available_contacts
                        .iter()
                        .map(|c| ToggleItem {
                            id: c.id.clone(),
                            label: c.name.clone(),
                            selected: self.config.selected_contact_ids.contains(&c.id),
                            subtitle: None,
                            a11y: Some(A11y::labeled(c.name.clone())),
                            info_key: None,
                        })
                        .collect(),
                    a11y: None,
                },
                Component::TextInput {
                    id: "alert_message".into(),
                    label: self.t("resistance.duress.alert_message"),
                    value: self.config.alert_message.clone(),
                    placeholder: Some(self.t("resistance.duress.message_placeholder")),
                    max_length: None,
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("resistance.duress.message_a11y")),
                        hint: Some(self.t("resistance.duress.message_placeholder")),
                        role: Some(AccessibilityRole::TextField),
                    }),
                    info_key: None,
                },
                Component::ToggleList {
                    id: "alerts".into(),
                    label: self.t("resistance.duress.options"),
                    items: vec![ToggleItem {
                        id: "include_location".into(),
                        label: self.t("resistance.duress.setup.include_location"),
                        selected: self.config.include_location,
                        subtitle: Some(self.t("resistance.duress.include_location_desc")),
                        a11y: Some(A11y::labeled(
                            self.t("resistance.duress.setup.include_location"),
                        )),
                        info_key: None,
                    }],
                    a11y: None,
                },
            ],
            contextual_actions: vec![
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
                    // A duress alert with no recipient reaches nobody — require ≥1
                    // whenever there is anybody to pick (2026-07-03-coercion-
                    // safety-config-gaps). With an empty pool the gate is
                    // unsatisfiable, and enforcing it would deny the PIN's local
                    // decoy protection to everyone who has not exchanged yet.
                    enabled: self.config.available_contacts.is_empty()
                        || !self.config.selected_contact_ids.is_empty(),
                    a11y: Some(A11y::labeled(self.t("action.save"))),
                },
            ],
            progress: Some(self.progress()),
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DuressPinEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        let config = self.config();
        Some(crate::ui::EngineOutput::DuressPin(
            crate::ui::DuressPinSetup {
                enabled: config.enabled,
                pin: self.pin().to_string(),
                alert_contact_ids: config.selected_contact_ids.clone(),
                alert_message: config.alert_message.clone(),
                include_location: config.include_location,
            },
        ))
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            DuressPinStep::Overview => self.overview_screen(),
            DuressPinStep::EnterPin => self.enter_pin_screen(),
            DuressPinStep::ConfirmPin => self.confirm_pin_screen(),
            DuressPinStep::ConfigureAlerts => self.alerts_screen(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // --- Overview ---
            (DuressPinStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "configure" =>
            {
                self.step = DuressPinStep::EnterPin;
                self.new_pin.clear();
                self.confirm_pin.clear();
                ActionResult::NavigateTo(self.current_screen())
            }
            (DuressPinStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "disable" =>
            {
                self.pending_disable = true;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "confirm_disable" =>
            {
                self.pending_disable = false;
                self.config.enabled = false;
                ActionResult::Complete
            }
            (DuressPinStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "cancel_disable" =>
            {
                self.pending_disable = false;
                ActionResult::UpdateScreen(self.current_screen())
            }

            // --- EnterPin ---
            (
                DuressPinStep::EnterPin,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "pin" => {
                if value.is_empty() {
                    self.new_pin.pop();
                } else {
                    self.new_pin = accept_pin_input(&self.new_pin, &value);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::EnterPin, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.new_pin.is_empty() {
                    ActionResult::ValidationError {
                        component_id: "pin".into(),
                        message: self.t("resistance.duress.error_empty"),
                    }
                } else if self.new_pin.len() < PIN_LENGTH {
                    // `error_too_short` shipped translated in every
                    // catalogue with nothing emitting it — the rule was
                    // designed and never wired. Advancing on a short PIN
                    // let core reject it later as a generic failure.
                    ActionResult::ValidationError {
                        component_id: "pin".into(),
                        message: get_string_with_args(
                            self.locale,
                            "resistance.duress.error_too_short",
                            &[("min", &PIN_LENGTH.to_string())],
                        ),
                    }
                } else {
                    self.step = DuressPinStep::ConfirmPin;
                    ActionResult::NavigateTo(self.current_screen())
                }
            }
            (DuressPinStep::EnterPin, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = DuressPinStep::Overview;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- ConfirmPin ---
            (
                DuressPinStep::ConfirmPin,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "confirm_pin" => {
                if value.is_empty() {
                    self.confirm_pin.pop();
                } else {
                    self.confirm_pin = accept_pin_input(&self.confirm_pin, &value);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::ConfirmPin, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.confirm_pin != self.new_pin {
                    ActionResult::ValidationError {
                        component_id: "confirm_pin".into(),
                        message: self.t("resistance.duress.error_mismatch"),
                    }
                } else {
                    self.step = DuressPinStep::ConfigureAlerts;
                    ActionResult::NavigateTo(self.current_screen())
                }
            }
            (DuressPinStep::ConfirmPin, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = DuressPinStep::EnterPin;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- ConfigureAlerts ---
            (
                DuressPinStep::ConfigureAlerts,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "alert_message" => {
                self.config.alert_message = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                DuressPinStep::ConfigureAlerts,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "recipients" => {
                if let Some(pos) = self
                    .config
                    .selected_contact_ids
                    .iter()
                    .position(|id| id == &item_id)
                {
                    self.config.selected_contact_ids.remove(pos);
                } else {
                    self.config.selected_contact_ids.push(item_id);
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            (
                DuressPinStep::ConfigureAlerts,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "alerts" && item_id == "include_location" => {
                self.config.include_location = !self.config.include_location;
                ActionResult::UpdateScreen(self.current_screen())
            }
            // Save is gated on ≥1 recipient; with none, this arm's guard fails
            // and the fallback re-renders (the disabled Save is a no-op). An
            // empty contact pool cannot satisfy that gate, so it is exempt —
            // the guard must mirror the Save affordance's enabled rule exactly,
            // or the button renders active and does nothing.
            (DuressPinStep::ConfigureAlerts, UserAction::ActionPressed { action_id })
                if (action_id == "save" || action_id == "submit_alert_message")
                    && (self.config.available_contacts.is_empty()
                        || !self.config.selected_contact_ids.is_empty()) =>
            {
                self.config.enabled = true;
                ActionResult::Complete
            }
            (DuressPinStep::ConfigureAlerts, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = DuressPinStep::ConfirmPin;
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- Fallback ---
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
