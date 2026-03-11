// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange engine — QR-based contact exchange workflow.

use crate::ui::*;

/// Configuration for starting an exchange.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ExchangeConfig {
    pub own_name: String,
    pub own_qr_data: String,
    /// Available groups for pre-selection (id, name). Empty = no group picker.
    #[serde(default)]
    pub available_groups: Vec<(String, String)>,
}

/// Engine that drives the QR exchange workflow.
pub struct ExchangeEngine {
    step: ExchangeStep,
    config: ExchangeConfig,
    scanned_data: Option<String>,
    /// Groups selected by the user before exchange.
    selected_groups: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum ExchangeStep {
    /// Pick groups for the new contact (shown only if groups exist).
    GroupSelection,
    ShowQr,
    ScanQr,
    Verifying,
    Success,
    Failed,
}

impl ExchangeStep {
    fn step_number(&self) -> u8 {
        match self {
            Self::GroupSelection => 1,
            Self::ShowQr => 2,
            Self::ScanQr => 3,
            Self::Verifying => 4,
            Self::Success => 5,
            Self::Failed => 6,
        }
    }
}

const TOTAL_STEPS: u8 = 6;

impl ExchangeEngine {
    pub fn new(config: ExchangeConfig) -> Self {
        let step = if config.available_groups.is_empty() {
            ExchangeStep::ShowQr
        } else {
            ExchangeStep::GroupSelection
        };
        Self {
            step,
            config,
            scanned_data: None,
            selected_groups: Vec::new(),
        }
    }

    /// Returns the groups selected by the user for the new contact.
    pub fn selected_groups(&self) -> &[String] {
        &self.selected_groups
    }

    /// Mark the exchange as successfully verified (called by the caller after
    /// crypto verification completes).
    pub fn mark_success(&mut self) {
        self.step = ExchangeStep::Success;
    }

    /// Mark the exchange as failed (called by the caller when verification
    /// fails).
    pub fn mark_failed(&mut self) {
        self.step = ExchangeStep::Failed;
    }

    /// Returns the data scanned from the peer's QR code, if any.
    pub fn scanned_data(&self) -> Option<&str> {
        self.scanned_data.as_deref()
    }

    fn progress(&self) -> Progress {
        Progress {
            current_step: self.step.step_number(),
            total_steps: TOTAL_STEPS,
            label: None,
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            ExchangeStep::GroupSelection => {
                let items: Vec<ToggleItem> = self
                    .config
                    .available_groups
                    .iter()
                    .map(|(id, name)| ToggleItem {
                        id: id.clone(),
                        label: name.clone(),
                        selected: self.selected_groups.contains(id),
                        subtitle: None,
                    })
                    .collect();
                ScreenModel {
                    screen_id: "exchange_group_selection".to_string(),
                    title: "Assign to Groups".to_string(),
                    subtitle: Some("Choose which groups the new contact will be in".to_string()),
                    components: vec![Component::ToggleList {
                        id: "group_picker".to_string(),
                        label: "Groups".to_string(),
                        items,
                    }],
                    actions: vec![
                        ScreenAction {
                            id: "continue".to_string(),
                            label: "Continue".to_string(),
                            style: ActionStyle::Primary,
                            enabled: true,
                        },
                        ScreenAction {
                            id: "skip".to_string(),
                            label: "Skip".to_string(),
                            style: ActionStyle::Secondary,
                            enabled: true,
                        },
                    ],
                    progress: Some(self.progress()),
                }
            }
            ExchangeStep::ShowQr => ScreenModel {
                screen_id: "exchange_show_qr".to_string(),
                title: "Share Your Code".to_string(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "own_qr".to_string(),
                    data: self.config.own_qr_data.clone(),
                    mode: QrMode::Display,
                    label: Some(self.config.own_name.clone()),
                }],
                actions: vec![ScreenAction {
                    id: "continue".to_string(),
                    label: "Scan Their Code".to_string(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::ScanQr => ScreenModel {
                screen_id: "exchange_scan_qr".to_string(),
                title: "Scan Their Code".to_string(),
                subtitle: None,
                components: vec![Component::QrCode {
                    id: "scan_qr".to_string(),
                    data: String::new(),
                    mode: QrMode::Scan,
                    label: None,
                }],
                actions: vec![ScreenAction {
                    id: "back".to_string(),
                    label: "Back".to_string(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::Verifying => ScreenModel {
                screen_id: "exchange_verifying".to_string(),
                title: "Verifying".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "verifying_status".to_string(),
                    icon: None,
                    title: "Verifying...".to_string(),
                    detail: None,
                    status: Status::InProgress,
                }],
                actions: vec![],
                progress: Some(self.progress()),
            },
            ExchangeStep::Success => ScreenModel {
                screen_id: "exchange_success".to_string(),
                title: "Success".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "success_status".to_string(),
                    icon: None,
                    title: "Exchange Complete".to_string(),
                    detail: None,
                    status: Status::Success,
                }],
                actions: vec![ScreenAction {
                    id: "done".to_string(),
                    label: "Done".to_string(),
                    style: ActionStyle::Primary,
                    enabled: true,
                }],
                progress: Some(self.progress()),
            },
            ExchangeStep::Failed => ScreenModel {
                screen_id: "exchange_failed".to_string(),
                title: "Failed".to_string(),
                subtitle: None,
                components: vec![Component::StatusIndicator {
                    id: "failed_status".to_string(),
                    icon: None,
                    title: "Exchange Failed".to_string(),
                    detail: None,
                    status: Status::Failed,
                }],
                actions: vec![
                    ScreenAction {
                        id: "retry".to_string(),
                        label: "Retry".to_string(),
                        style: ActionStyle::Primary,
                        enabled: true,
                    },
                    ScreenAction {
                        id: "cancel".to_string(),
                        label: "Cancel".to_string(),
                        style: ActionStyle::Secondary,
                        enabled: true,
                    },
                ],
                progress: Some(self.progress()),
            },
        }
    }
}

impl WorkflowEngine for ExchangeEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Group selection: toggle group membership
            (
                ExchangeStep::GroupSelection,
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "group_picker" => {
                if let Some(pos) = self.selected_groups.iter().position(|g| g == &item_id) {
                    self.selected_groups.remove(pos);
                } else {
                    self.selected_groups.push(item_id);
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            // Group selection: continue or skip
            (ExchangeStep::GroupSelection, UserAction::ActionPressed { action_id })
                if action_id == "continue" || action_id == "skip" =>
            {
                if action_id == "skip" {
                    self.selected_groups.clear();
                }
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::ShowQr, UserAction::ActionPressed { action_id })
                if action_id == "continue" =>
            {
                self.step = ExchangeStep::ScanQr;
                ActionResult::RequestCamera
            }
            (ExchangeStep::ScanQr, UserAction::ActionPressed { action_id })
                if action_id == "back" =>
            {
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (
                ExchangeStep::ScanQr,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == "scanned_data" => {
                self.scanned_data = Some(value);
                self.step = ExchangeStep::Verifying;
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Success, UserAction::ActionPressed { action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "retry" =>
            {
                self.scanned_data = None;
                self.step = ExchangeStep::ShowQr;
                ActionResult::NavigateTo(self.build_screen())
            }
            (ExchangeStep::Failed, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: Tests access private ExchangeStep enum and ExchangeEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    fn config_no_groups() -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr-data".into(),
            available_groups: vec![],
        }
    }

    fn config_with_groups() -> ExchangeConfig {
        ExchangeConfig {
            own_name: "Alice".into(),
            own_qr_data: "qr-data".into(),
            available_groups: vec![
                ("g1".into(), "Family".into()),
                ("g2".into(), "Friends".into()),
            ],
        }
    }

    #[test]
    fn test_no_groups_skips_selection() {
        let engine = ExchangeEngine::new(config_no_groups());
        // Should start directly at ShowQr when no groups available
        assert_eq!(engine.step, ExchangeStep::ShowQr);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_show_qr");
    }

    #[test]
    fn test_with_groups_starts_at_selection() {
        let engine = ExchangeEngine::new(config_with_groups());
        // Should start at GroupSelection when groups exist
        assert_eq!(engine.step, ExchangeStep::GroupSelection);
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "exchange_group_selection");
    }

    #[test]
    fn test_group_selection_toggle_and_continue() {
        let mut engine = ExchangeEngine::new(config_with_groups());

        // Toggle first group on
        let result = engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g1".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));

        // Continue to ShowQr
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::ShowQr);

        // Selected groups should be remembered
        assert_eq!(engine.selected_groups(), &["g1".to_string()]);
    }

    #[test]
    fn test_group_selection_skip() {
        let mut engine = ExchangeEngine::new(config_with_groups());

        // Skip without selecting any groups
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "skip".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.step, ExchangeStep::ShowQr);
        assert!(engine.selected_groups().is_empty());
    }

    #[test]
    fn test_selected_groups_persists_through_exchange() {
        let mut engine = ExchangeEngine::new(config_with_groups());

        // Select a group
        engine.handle_action(UserAction::ItemToggled {
            component_id: "group_picker".into(),
            item_id: "g2".into(),
        });
        engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });

        // Continue through ShowQr → ScanQr → Verifying → Success
        engine.handle_action(UserAction::ActionPressed {
            action_id: "continue".into(),
        });
        engine.handle_action(UserAction::TextChanged {
            component_id: "scanned_data".into(),
            value: "their-qr".into(),
        });
        engine.mark_success();

        // Groups still selected at the end
        assert_eq!(engine.selected_groups(), &["g2".to_string()]);
    }
}
