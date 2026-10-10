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

/// What a PIN field holds after an edit; `value` is the whole field.
///
/// The field shows the digits Core holds as `•` (the `PinInput`
/// projection), so leading bullets stand for those digits — a shell that
/// echoes Core's value sends `••5` — and the digits after them are typed. A
/// shell that keeps its own text (Android) sends only digits, the whole PIN
/// so far. Appending every value instead stored `665654` for `654321`
/// (vauchi/private#618). Non-digits are dropped and the length capped, so
/// a pasted `undefined` still cannot become a credential (#202).
fn pin_from_field(held: &str, value: &str) -> String {
    let kept = value
        .chars()
        .take_while(|c| *c == '•')
        .count()
        .min(held.len());
    let mut out = String::from(&held[..kept]);
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
    /// Number of decoy contacts, for the overview's decoy row.
    decoy_count: usize,
    /// Alerts opened from the overview's alerts row: no PIN is asked, Back
    /// returns to the overview, and saving leaves the PIN as it is.
    alerts_only: bool,
    locale: Locale,
}

const ROWS_ID: &str = "duress_rows";
const ACTIONS_ID: &str = "duress_actions";

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
            decoy_count: 0,
            alerts_only: false,
            locale,
        }
    }

    /// Set the number of decoy contacts shown on the overview.
    pub fn with_decoy_count(mut self, count: usize) -> Self {
        self.decoy_count = count;
        self
    }

    fn count(&self, n: usize, none: &str, one: &str, many: &str) -> String {
        match n {
            0 => self.t(none),
            1 => self.t(one),
            _ => get_string_with_args(self.locale, many, &[("count", &n.to_string())]),
        }
    }

    fn start_pin_entry(&mut self) -> ActionResult {
        self.step = DuressPinStep::EnterPin;
        self.alerts_only = false;
        self.new_pin.clear();
        self.confirm_pin.clear();
        ActionResult::NavigateTo(self.current_screen())
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

    /// Setup is three steps; the overview is not one of them, and editing
    /// only the alerts is a single screen without a step bar (#459).
    fn progress(&self) -> Option<Progress> {
        let current_step = match self.step {
            DuressPinStep::Overview => return None,
            DuressPinStep::ConfigureAlerts if self.alerts_only => return None,
            DuressPinStep::EnterPin => 1,
            DuressPinStep::ConfirmPin => 2,
            DuressPinStep::ConfigureAlerts => 3,
        };
        Some(Progress {
            current_step,
            total_steps: 3,
            label: None,
        })
    }

    fn overview_screen(&self) -> ScreenModel {
        let mut components = vec![Component::Text {
            id: "duress_intro".into(),
            content: self.t("resistance.duress.what_is_detail"),
            style: TextStyle::Body,
            a11y: None,
        }];

        let button = |id: &str, key: &str| ActionListItem {
            id: id.into(),
            label: self.t(key),
            icon: None,
            detail: None,
            a11y: None,
            info_key: None,
        };

        if self.config.enabled {
            let row = |id: &str, key: &str, detail: String| ActionListItem {
                id: id.into(),
                label: self.t(key),
                icon: None,
                detail: Some(detail),
                a11y: None,
                info_key: None,
            };
            components.push(Component::ActionList {
                id: ROWS_ID.into(),
                items: vec![
                    row(
                        "pin",
                        "resistance.duress.row_pin",
                        self.t("resistance.duress.row_pin_set"),
                    ),
                    row(
                        "decoys",
                        "resistance.duress.row_decoys",
                        self.count(
                            self.decoy_count,
                            "resistance.duress.row_decoys_none",
                            "resistance.duress.row_decoys_one",
                            "resistance.duress.row_decoys_many",
                        ),
                    ),
                    row(
                        "alerts",
                        "resistance.duress.row_alerts",
                        self.count(
                            self.config.selected_contact_ids.len(),
                            "resistance.duress.row_alerts_off",
                            "resistance.duress.row_alerts_one",
                            "resistance.duress.row_alerts_many",
                        ),
                    ),
                ],
            });
            components.push(Component::ButtonList {
                id: ACTIONS_ID.into(),
                items: vec![
                    button("how_it_looks", "resistance.duress.how_it_looks_button"),
                    button("turn_off", "resistance.duress.turn_off_button"),
                ],
            });
        } else {
            // A read-only status states "not set up" plainly; a toggle here
            // would be a control that cannot act (2026-07-03 config gaps).
            components.push(Component::StatusIndicator {
                id: "duress_status".into(),
                icon: Some("exclamationmark.shield".into()),
                title: self.t("resistance.duress.not_set_up"),
                detail: None,
                status: Status::Warning,
                status_label: self.t(Status::Warning.label_key()),
                a11y: None,
            });
            components.push(Component::ButtonList {
                id: ACTIONS_ID.into(),
                items: vec![button("set_up", "resistance.duress.set_up_pin")],
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
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }

    fn enter_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_enter_pin".into(),
            title: self.t("resistance.duress.setup"),
            subtitle: None,
            components: vec![
                Component::PinInput {
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
                },
                // Owner decision 2026-10-01 (#462): say it at setup.
                Component::Text {
                    id: "shred_note".into(),
                    content: self.t("resistance.duress.shred_note"),
                    style: TextStyle::Caption,
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
                    id: "continue".into(),
                    label: self.t("action.continue"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.continue"))),
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn confirm_pin_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "duress_confirm_pin".into(),
            // Titled like the entry step rather than repeating the field's
            // own label: shells derive the surface's accessibility label from
            // the title, so reusing `confirm_pin` here put the same string on
            // the surface, the heading, the input and the input's label. A
            // screen reader then announced "Confirm Duress PIN" four times
            // with nothing to distinguish the field, and anything selecting by
            // label reached the surface instead of the input.
            title: self.t("resistance.duress.setup"),
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
            progress: self.progress(),
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
            progress: self.progress(),
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
                // Empty when only the alerts were edited: the PIN stays.
                pin: if self.alerts_only {
                    String::new()
                } else {
                    self.pin().to_string()
                },
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
            (
                DuressPinStep::Overview,
                UserAction::ListItemSelected {
                    component_id,
                    item_id,
                },
            ) if (component_id == ACTIONS_ID && item_id == "set_up")
                || (component_id == ROWS_ID && item_id == "pin") =>
            {
                self.start_pin_entry()
            }
            (
                DuressPinStep::Overview,
                UserAction::ListItemSelected {
                    component_id,
                    item_id,
                },
            ) if component_id == ROWS_ID && item_id == "alerts" => {
                self.step = DuressPinStep::ConfigureAlerts;
                self.alerts_only = true;
                ActionResult::NavigateTo(self.current_screen())
            }
            (
                DuressPinStep::Overview,
                UserAction::ListItemSelected {
                    component_id,
                    item_id,
                },
            ) if component_id == ACTIONS_ID && item_id == "how_it_looks" => {
                ActionResult::ShowInfoOverlay {
                    title: self.t("resistance.duress.how_it_looks_button"),
                    body: self.t("resistance.duress.how_it_looks_body"),
                }
            }
            (
                DuressPinStep::Overview,
                UserAction::ListItemSelected {
                    component_id,
                    item_id,
                },
            ) if component_id == ACTIONS_ID && item_id == "turn_off" => {
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
                let pin = pin_from_field(&self.new_pin, &value);
                self.new_pin.zeroize();
                self.new_pin = pin;
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
                let pin = pin_from_field(&self.confirm_pin, &value);
                self.confirm_pin.zeroize();
                self.confirm_pin = pin;
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
                self.step = if self.alerts_only {
                    self.alerts_only = false;
                    DuressPinStep::Overview
                } else {
                    DuressPinStep::ConfirmPin
                };
                ActionResult::NavigateTo(self.current_screen())
            }

            // --- Fallback ---
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
