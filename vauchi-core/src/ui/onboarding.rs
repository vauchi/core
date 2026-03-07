// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding workflow engine — a pure state machine for the 9-screen
//! onboarding flow.  No Storage or Vauchi dependency; the caller
//! persists results when [`ActionResult::Complete`] is returned.

use crate::contact::labels::SUGGESTED_LABELS;
use crate::ui::*;

// ── Internal step enum (private) ────────────────────────────────────

#[derive(Clone, Debug)]
enum Step {
    Welcome,
    DefaultName,
    SkipGate,
    GroupsSetup,
    ContactInfo,
    PreviewCard,
    SecurityExplanation,
    BackupPrompt,
    Ready,
}

// ── Public data types ───────────────────────────────────────────────

/// Data collected during onboarding.
#[derive(Clone, Debug, Default)]
pub struct OnboardingData {
    pub display_name: String,
    pub selected_groups: Vec<GroupSetup>,
    pub fields: Vec<FieldSetup>,
}

/// A group the user can toggle during onboarding.
#[derive(Clone, Debug)]
pub struct GroupSetup {
    pub name: String,
    pub selected: bool,
    pub name_override: Option<String>,
}

/// A contact field configured during onboarding.
#[derive(Clone, Debug)]
pub struct FieldSetup {
    pub field_type: String,
    pub label: String,
    pub value: String,
    pub visible_to_groups: Vec<String>,
    pub shown: bool,
}

// ── OnboardingEngine ────────────────────────────────────────────────

/// Pure state-machine driving the 9-screen onboarding flow.
#[derive(Clone, Debug)]
pub struct OnboardingEngine {
    step: Step,
    data: OnboardingData,
    selected_preview_group: Option<String>,
}

impl Default for OnboardingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingEngine {
    /// Creates a new engine starting at the Welcome screen.
    pub fn new() -> Self {
        let groups = SUGGESTED_LABELS
            .iter()
            .map(|&name| GroupSetup {
                name: name.to_string(),
                selected: false,
                name_override: None,
            })
            .collect();

        Self {
            step: Step::Welcome,
            data: OnboardingData {
                display_name: String::new(),
                selected_groups: groups,
                fields: Vec::new(),
            },
            selected_preview_group: None,
        }
    }

    /// Returns a reference to the collected onboarding data.
    pub fn data(&self) -> &OnboardingData {
        &self.data
    }

    // ── Screen builders ─────────────────────────────────────────────

    fn progress(&self, step: u8) -> Option<Progress> {
        Some(Progress {
            current_step: step,
            total_steps: 9,
            label: None,
        })
    }

    fn build_welcome(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "welcome".into(),
            title: "Welcome to Vauchi".into(),
            subtitle: Some("Your contacts, your rules.".into()),
            components: vec![Component::InfoPanel {
                id: "value_proposition".into(),
                icon: None,
                title: "Why Vauchi?".into(),
                items: vec![
                    InfoItem {
                        icon: Some("lock".into()),
                        title: "Private".into(),
                        detail: "End-to-end encrypted contact cards".into(),
                    },
                    InfoItem {
                        icon: Some("refresh".into()),
                        title: "Always up to date".into(),
                        detail: "Update your card and contacts see changes automatically".into(),
                    },
                    InfoItem {
                        icon: Some("people".into()),
                        title: "You decide who sees what".into(),
                        detail: "Share different info with different groups".into(),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "get_started".into(),
                    label: "Get Started".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "restore_backup".into(),
                    label: "Restore Backup".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(1),
        }
    }

    fn build_default_name(&self) -> ScreenModel {
        let name_filled = !self.data.display_name.trim().is_empty();
        ScreenModel {
            screen_id: "default_name".into(),
            title: "What's your name?".into(),
            subtitle: Some("This is how contacts will see you.".into()),
            components: vec![
                Component::Text {
                    id: "name_instruction".into(),
                    content: "Enter the name you'd like to show on your contact card.".into(),
                    style: TextStyle::Body,
                },
                Component::TextInput {
                    id: "display_name".into(),
                    label: "Display name".into(),
                    value: self.data.display_name.clone(),
                    placeholder: Some("Enter your name".into()),
                    max_length: Some(100),
                    validation_error: None,
                    input_type: InputType::Text,
                },
            ],
            actions: vec![ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: name_filled,
            }],
            progress: self.progress(2),
        }
    }

    fn build_skip_gate(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "skip_gate".into(),
            title: "Set up your card".into(),
            subtitle: Some("A few more steps to get the most out of Vauchi.".into()),
            components: vec![Component::InfoPanel {
                id: "skip_info".into(),
                icon: None,
                title: "What you'll set up next".into(),
                items: vec![
                    InfoItem {
                        icon: Some("group".into()),
                        title: "Groups".into(),
                        detail: "Organize contacts into groups like Family, Friends, etc.".into(),
                    },
                    InfoItem {
                        icon: Some("card".into()),
                        title: "Contact info".into(),
                        detail: "Add phone, email, and other fields to your card.".into(),
                    },
                    InfoItem {
                        icon: Some("eye".into()),
                        title: "Visibility".into(),
                        detail: "Choose what each group can see.".into(),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "continue_setup".into(),
                    label: "Continue setup".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "skip_to_finish".into(),
                    label: "Skip to finish".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(3),
        }
    }

    fn build_groups_setup(&self) -> ScreenModel {
        let items = self
            .data
            .selected_groups
            .iter()
            .map(|g| ToggleItem {
                id: g.name.clone(),
                label: g.name.clone(),
                selected: g.selected,
                subtitle: g.name_override.clone(),
            })
            .collect();

        ScreenModel {
            screen_id: "groups_setup".into(),
            title: "Choose your groups".into(),
            subtitle: Some("Groups let you share different info with different people.".into()),
            components: vec![Component::ToggleList {
                id: "groups".into(),
                label: "Suggested groups".into(),
                items,
            }],
            actions: vec![
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "skip".into(),
                    label: "Skip".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(4),
        }
    }

    fn selected_group_names(&self) -> Vec<String> {
        self.data
            .selected_groups
            .iter()
            .filter(|g| g.selected)
            .map(|g| g.name.clone())
            .collect()
    }

    fn build_contact_info(&self) -> ScreenModel {
        let selected = self.selected_group_names();
        let has_groups = !selected.is_empty();

        let fields: Vec<FieldDisplay> = self
            .data
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldDisplay {
                id: format!("field_{i}"),
                field_type: f.field_type.clone(),
                label: f.label.clone(),
                value: f.value.clone(),
                visibility: if f.shown {
                    if has_groups {
                        UiFieldVisibility::Groups(f.visible_to_groups.clone())
                    } else {
                        UiFieldVisibility::Shown
                    }
                } else {
                    UiFieldVisibility::Hidden
                },
            })
            .collect();

        ScreenModel {
            screen_id: "contact_info".into(),
            title: "Add contact info".into(),
            subtitle: Some("Add fields to your contact card.".into()),
            components: vec![Component::FieldList {
                id: "fields".into(),
                fields,
                visibility_mode: if has_groups {
                    VisibilityMode::PerGroup
                } else {
                    VisibilityMode::ShowHide
                },
                available_groups: selected,
            }],
            actions: vec![
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "skip".into(),
                    label: "Skip".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(5),
        }
    }

    fn build_preview_card(&self) -> ScreenModel {
        let selected = self.selected_group_names();

        let fields: Vec<FieldDisplay> = self
            .data
            .fields
            .iter()
            .enumerate()
            .map(|(i, f)| FieldDisplay {
                id: format!("field_{i}"),
                field_type: f.field_type.clone(),
                label: f.label.clone(),
                value: f.value.clone(),
                visibility: if f.shown {
                    UiFieldVisibility::Shown
                } else {
                    UiFieldVisibility::Hidden
                },
            })
            .collect();

        let group_views: Vec<GroupCardView> = selected
            .iter()
            .map(|group_name| {
                let visible_fields: Vec<FieldDisplay> = self
                    .data
                    .fields
                    .iter()
                    .enumerate()
                    .filter(|(_, f)| f.shown && f.visible_to_groups.contains(group_name))
                    .map(|(i, f)| FieldDisplay {
                        id: format!("field_{i}"),
                        field_type: f.field_type.clone(),
                        label: f.label.clone(),
                        value: f.value.clone(),
                        visibility: UiFieldVisibility::Shown,
                    })
                    .collect();

                let display_name = self
                    .data
                    .selected_groups
                    .iter()
                    .find(|g| &g.name == group_name)
                    .and_then(|g| g.name_override.clone())
                    .unwrap_or_else(|| self.data.display_name.clone());

                GroupCardView {
                    group_name: group_name.clone(),
                    display_name,
                    visible_fields,
                }
            })
            .collect();

        ScreenModel {
            screen_id: "preview_card".into(),
            title: "Preview your card".into(),
            subtitle: Some("This is how your card will look.".into()),
            components: vec![Component::CardPreview {
                name: self.data.display_name.clone(),
                fields,
                group_views,
                selected_group: self.selected_preview_group.clone(),
            }],
            actions: vec![
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "edit".into(),
                    label: "Edit".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(6),
        }
    }

    fn build_security_explanation(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "security_explanation".into(),
            title: "Your data is secure".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "security_features".into(),
                icon: Some("shield".into()),
                title: "How Vauchi protects you".into(),
                items: vec![
                    InfoItem {
                        icon: Some("lock".into()),
                        title: "End-to-end encryption".into(),
                        detail: "Only you and your contacts can read your data.".into(),
                    },
                    InfoItem {
                        icon: Some("server".into()),
                        title: "Decentralized".into(),
                        detail: "No central server stores your contacts.".into(),
                    },
                    InfoItem {
                        icon: Some("key".into()),
                        title: "You own your keys".into(),
                        detail: "Your cryptographic keys never leave your device.".into(),
                    },
                ],
            }],
            actions: vec![ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: self.progress(7),
        }
    }

    fn build_backup_prompt(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "backup_prompt".into(),
            title: "Back up your identity".into(),
            subtitle: None,
            components: vec![Component::InfoPanel {
                id: "backup_info".into(),
                icon: Some("backup".into()),
                title: "Why back up?".into(),
                items: vec![
                    InfoItem {
                        icon: Some("warning".into()),
                        title: "No account recovery".into(),
                        detail: "If you lose your device without a backup, your identity is lost."
                            .into(),
                    },
                    InfoItem {
                        icon: Some("devices".into()),
                        title: "Multi-device".into(),
                        detail: "Backups let you use Vauchi on multiple devices.".into(),
                    },
                ],
            }],
            actions: vec![
                ScreenAction {
                    id: "setup_backup".into(),
                    label: "Set up backup".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                },
                ScreenAction {
                    id: "skip".into(),
                    label: "Skip".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                },
            ],
            progress: self.progress(8),
        }
    }

    fn build_ready(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "ready".into(),
            title: "You're all set!".into(),
            subtitle: Some("Your Vauchi identity is ready.".into()),
            components: vec![Component::InfoPanel {
                id: "ready_info".into(),
                icon: Some("check".into()),
                title: "What's next".into(),
                items: vec![
                    InfoItem {
                        icon: Some("share".into()),
                        title: "Share your card".into(),
                        detail: "Exchange contact cards in person.".into(),
                    },
                    InfoItem {
                        icon: Some("edit".into()),
                        title: "Edit anytime".into(),
                        detail: "Update your card whenever you like.".into(),
                    },
                ],
            }],
            actions: vec![ScreenAction {
                id: "start".into(),
                label: "Start using Vauchi".into(),
                style: ActionStyle::Primary,
                enabled: true,
            }],
            progress: self.progress(9),
        }
    }

    // ── Action handlers ─────────────────────────────────────────────

    fn navigate_to(&mut self, step: Step) -> ActionResult {
        self.step = step;
        ActionResult::NavigateTo(self.current_screen())
    }

    fn handle_welcome(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "get_started" => {
                self.navigate_to(Step::DefaultName)
            }
            // restore_backup would be handled externally; return the screen unchanged
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_default_name(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "display_name" => {
                self.data.display_name = value.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "continue" => {
                if self.data.display_name.trim().is_empty() {
                    ActionResult::ValidationError {
                        component_id: "display_name".into(),
                        message: "Please enter your name.".into(),
                    }
                } else {
                    self.navigate_to(Step::SkipGate)
                }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_skip_gate(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "continue_setup" => {
                self.navigate_to(Step::GroupsSetup)
            }
            UserAction::ActionPressed { action_id } if action_id == "skip_to_finish" => {
                self.navigate_to(Step::SecurityExplanation)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_groups_setup(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ItemToggled {
                component_id,
                item_id,
            } if component_id == "groups" => {
                if let Some(group) = self
                    .data
                    .selected_groups
                    .iter_mut()
                    .find(|g| g.name == *item_id)
                {
                    group.selected = !group.selected;
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id.starts_with("group_name_override_") => {
                let id = component_id.strip_prefix("group_name_override_").unwrap();
                if let Some(group) = self.data.selected_groups.iter_mut().find(|g| g.name == id) {
                    group.name_override = if value.trim().is_empty() {
                        None
                    } else {
                        Some(value.clone())
                    };
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id == "continue" || action_id == "skip" =>
            {
                self.navigate_to(Step::ContactInfo)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_contact_info(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::FieldVisibilityChanged {
                field_id,
                group_id,
                visible,
            } => {
                if let Some(idx) = field_id
                    .strip_prefix("field_")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    if let Some(field) = self.data.fields.get_mut(idx) {
                        match group_id {
                            Some(group) => {
                                if *visible {
                                    if !field.visible_to_groups.contains(group) {
                                        field.visible_to_groups.push(group.clone());
                                    }
                                } else {
                                    field.visible_to_groups.retain(|g| g != group);
                                }
                            }
                            None => {
                                field.shown = *visible;
                            }
                        }
                    }
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id == "continue" || action_id == "skip" =>
            {
                self.navigate_to(Step::PreviewCard)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_preview_card(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "continue" => {
                self.navigate_to(Step::SecurityExplanation)
            }
            UserAction::ActionPressed { action_id } if action_id == "edit" => {
                self.navigate_to(Step::ContactInfo)
            }
            UserAction::GroupViewSelected { group_name } => {
                self.selected_preview_group = group_name.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_security_explanation(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "continue" => {
                self.navigate_to(Step::BackupPrompt)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_backup_prompt(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id }
                if action_id == "setup_backup" || action_id == "skip" =>
            {
                self.navigate_to(Step::Ready)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_ready(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "start" => {
                ActionResult::Complete
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

impl WorkflowEngine for OnboardingEngine {
    fn current_screen(&self) -> ScreenModel {
        match self.step {
            Step::Welcome => self.build_welcome(),
            Step::DefaultName => self.build_default_name(),
            Step::SkipGate => self.build_skip_gate(),
            Step::GroupsSetup => self.build_groups_setup(),
            Step::ContactInfo => self.build_contact_info(),
            Step::PreviewCard => self.build_preview_card(),
            Step::SecurityExplanation => self.build_security_explanation(),
            Step::BackupPrompt => self.build_backup_prompt(),
            Step::Ready => self.build_ready(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match self.step {
            Step::Welcome => self.handle_welcome(&action),
            Step::DefaultName => self.handle_default_name(&action),
            Step::SkipGate => self.handle_skip_gate(&action),
            Step::GroupsSetup => self.handle_groups_setup(&action),
            Step::ContactInfo => self.handle_contact_info(&action),
            Step::PreviewCard => self.handle_preview_card(&action),
            Step::SecurityExplanation => self.handle_security_explanation(&action),
            Step::BackupPrompt => self.handle_backup_prompt(&action),
            Step::Ready => self.handle_ready(&action),
        }
    }
}
