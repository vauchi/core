// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Duress PIN engine — configure a duress PIN that triggers silent alerts.

use crate::ui::*;
use serde::{Deserialize, Serialize};

/// Configuration for the duress PIN feature.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DuressConfig {
    pub enabled: bool,
    pub alert_contacts: Vec<ContactItem>,
    pub alert_message: String,
    pub include_location: bool,
}

/// Engine that drives the duress PIN setup workflow.
pub struct DuressPinEngine {
    step: DuressPinStep,
    config: DuressConfig,
    new_pin: String,
    confirm_pin: String,
}

#[derive(Clone, Debug, PartialEq)]
enum DuressPinStep {
    Overview,
    EnterPin,
    ConfirmPin,
    ConfigureAlerts,
}

impl DuressPinEngine {
    pub fn new(config: DuressConfig) -> Self {
        Self {
            step: DuressPinStep::Overview,
            config,
            new_pin: String::new(),
            confirm_pin: String::new(),
        }
    }

    pub fn config(&self) -> &DuressConfig {
        &self.config
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
        let components = vec![
            Component::InfoPanel {
                id: "duress_info".into(),
                icon: Some("shield".into()),
                title: "Duress PIN".into(),
                items: vec![InfoItem {
                    icon: Some("info".into()),
                    title: "What is a Duress PIN?".into(),
                    detail: "A secondary PIN that, when entered, silently alerts \
                             your chosen contacts while appearing to unlock normally."
                        .into(),
                }],
            },
            Component::ToggleList {
                id: "duress_toggle".into(),
                label: "Status".into(),
                items: vec![ToggleItem {
                    id: "enabled".into(),
                    label: "Duress PIN enabled".into(),
                    selected: self.config.enabled,
                    subtitle: None,
                }],
            },
        ];

        let mut actions = vec![ScreenAction {
            id: "configure".into(),
            label: if self.config.enabled {
                "Change PIN".into()
            } else {
                "Set Up PIN".into()
            },
            style: ActionStyle::Primary,
            enabled: true,
        }];

        if self.config.enabled {
            actions.push(ScreenAction {
                id: "disable".into(),
                label: "Disable".into(),
                style: ActionStyle::Destructive,
                enabled: true,
            });
        }

        ScreenModel {
            screen_id: "duress_overview".into(),
            title: "Duress PIN".into(),
            subtitle: None,
            components,
            actions,
            progress: Some(self.progress()),
        }
    }

    fn enter_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_enter_pin".into(),
            title: "Set Duress PIN".into(),
            subtitle: None,
            components: vec![Component::PinInput {
                id: "pin".into(),
                label: "Enter Duress PIN".into(),
                length: 6,
                masked: true,
                validation_error: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: Some(self.progress()),
        }
    }

    fn confirm_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_confirm_pin".into(),
            title: "Confirm Duress PIN".into(),
            subtitle: None,
            components: vec![Component::PinInput {
                id: "confirm_pin".into(),
                label: "Confirm Duress PIN".into(),
                length: 6,
                masked: true,
                validation_error: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: Some(self.progress()),
        }
    }

    fn alerts_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_alerts".into(),
            title: "Configure Alerts".into(),
            subtitle: None,
            components: vec![
                Component::TextInput {
                    id: "alert_message".into(),
                    label: "Alert Message".into(),
                    value: self.config.alert_message.clone(),
                    placeholder: Some("Message to send when duress PIN is used".into()),
                    max_length: None,
                    validation_error: None,
                    input_type: InputType::Text,
                },
                Component::ToggleList {
                    id: "alerts".into(),
                    label: "Options".into(),
                    items: vec![ToggleItem {
                        id: "include_location".into(),
                        label: "Include Location".into(),
                        selected: self.config.include_location,
                        subtitle: Some("Share your location in the alert".into()),
                    }],
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
                ScreenAction {
                    id: "save".into(),
                    label: "Save".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
            ],
            progress: Some(self.progress()),
        }
    }
}

impl WorkflowEngine for DuressPinEngine {
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
                self.config.enabled = false;
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
                self.new_pin = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::EnterPin, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.new_pin.is_empty() {
                    ActionResult::ValidationError {
                        component_id: "pin".into(),
                        message: "Please enter a PIN".into(),
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
                self.confirm_pin = value;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::ConfirmPin, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                if self.confirm_pin != self.new_pin {
                    ActionResult::ValidationError {
                        component_id: "confirm_pin".into(),
                        message: "PINs do not match".into(),
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
            ) if component_id == "alerts" && item_id == "include_location" => {
                self.config.include_location = !self.config.include_location;
                ActionResult::UpdateScreen(self.current_screen())
            }
            (DuressPinStep::ConfigureAlerts, UserAction::ActionPressed { action_id })
                if action_id == "save" =>
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
