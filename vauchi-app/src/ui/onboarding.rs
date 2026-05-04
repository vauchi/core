// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding workflow engine — a pure state machine for the 6-step
//! onboarding flow (2 pre-gate + 4 main).  No Storage or Vauchi
//! dependency; the caller persists results when [`ActionResult::Complete`]
//! is returned.

use crate::ui::*;
use vauchi_core::contact::labels::SUGGESTED_LABELS;
use vauchi_core::types::OnboardingStep as Step;
use vauchi_core::{Command, FilePickPurpose};

/// `accepted_mime_types` for the encrypted backup file picker. Frontends
/// may default to a coarser superset on platforms where the OS picker
/// doesn't filter by MIME (older Android variants).
fn backup_mime_types() -> Vec<String> {
    vec!["application/octet-stream".into(), "text/plain".into()]
}

// ── Public data types ───────────────────────────────────────────────

/// Data collected during onboarding.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct OnboardingData {
    pub display_name: String,
    pub selected_groups: Vec<GroupSetup>,
    pub fields: Vec<FieldSetup>,
}

/// A group the user can toggle during onboarding.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GroupSetup {
    pub name: String,
    pub selected: bool,
    pub name_override: Option<String>,
}

/// A contact field configured during onboarding.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct FieldSetup {
    pub field_type: String,
    pub label: String,
    pub value: String,
    pub visible_to_groups: Vec<String>,
    pub shown: bool,
}

// ── OnboardingEngine ────────────────────────────────────────────────

/// Pure state-machine driving the 6-step onboarding flow.
#[derive(Clone, Debug)]
pub struct OnboardingEngine {
    step: Step,
    data: OnboardingData,
    custom_group_input: String,
    phone_input_visible: bool,
    email_input_visible: bool,
    phone_value: String,
    email_value: String,
    /// When true, info icon keys are set on components that have help content.
    show_help_icons: bool,
    /// Bytes of the encrypted backup file picked via
    /// `Command::FilePickFromUser` while restoring an identity.
    /// Set by `set_pending_backup_bytes` from the AppEngine
    /// `handle_file_picked` Onboarding arm; consumed by the routing
    /// layer's completion path when the user submits the password.
    pending_backup_bytes: Option<Vec<u8>>,
    /// Password the user typed on the BackupPasswordEntry screen.
    /// Used in concert with `pending_backup_bytes` at submit time.
    pending_backup_password: String,
}

impl Default for OnboardingEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl OnboardingEngine {
    /// Creates a new engine starting at the IdentityCheck screen.
    ///
    /// Help icons are disabled by default. Use [`with_help_icons`](Self::with_help_icons)
    /// to enable them.
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
            step: Step::IdentityCheck,
            data: OnboardingData {
                display_name: String::new(),
                selected_groups: groups,
                fields: Vec::new(),
            },
            custom_group_input: String::new(),
            phone_input_visible: false,
            email_input_visible: false,
            phone_value: String::new(),
            email_value: String::new(),
            show_help_icons: false,
            pending_backup_bytes: None,
            pending_backup_password: String::new(),
        }
    }

    /// Stores the picked backup bytes and transitions the wizard to
    /// the password-entry step. Called by `AppEngine::handle_file_picked`
    /// (Onboarding arm) when the user picks a file via the
    /// `Command::FilePickFromUser{purpose:ImportBackup}` flow.
    pub fn set_pending_backup_bytes(&mut self, bytes: Vec<u8>) {
        self.pending_backup_bytes = Some(bytes);
        self.pending_backup_password.clear();
        self.step = Step::BackupPasswordEntry;
    }

    /// Returns `Some(bytes, password)` when the user has submitted the
    /// password on `BackupPasswordEntry`. Both the bytes and the
    /// password are taken (cleared on the engine), so re-submitting
    /// without re-picking the file is impossible. Used by the AppEngine
    /// completion path to call `Vauchi::import_full_backup`.
    pub fn take_pending_backup(&mut self) -> Option<(Vec<u8>, String)> {
        let bytes = self.pending_backup_bytes.take()?;
        let password = std::mem::take(&mut self.pending_backup_password);
        Some((bytes, password))
    }

    /// Returns the current step. Used by the AppEngine completion path
    /// to detect a `BackupPasswordEntry` submit (the engine re-uses
    /// `ActionResult::Complete` for the submit signal — completion
    /// handler routes by step).
    pub fn current_step(&self) -> Step {
        self.step
    }

    /// Enable or disable help icons on onboarding components.
    ///
    /// When `true`, components that have associated help content will have
    /// `info_key` set so the frontend can render an info icon that opens a
    /// help overlay.
    pub fn with_help_icons(mut self, enabled: bool) -> Self {
        self.show_help_icons = enabled;
        self
    }

    /// Returns a reference to the collected onboarding data.
    pub fn data(&self) -> &OnboardingData {
        &self.data
    }

    // ── Screen builders ─────────────────────────────────────────────

    fn progress(&self, step: u8) -> Option<Progress> {
        Some(Progress {
            current_step: step,
            total_steps: 4,
            label: None,
        })
    }

    fn build_identity_check(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "identity_check".into(),
            title: "Welcome to Vauchi".into(),
            subtitle: Some("Privacy-focused contact cards.".into()),
            components: vec![Component::InfoPanel {
                id: "identity_check_info".into(),
                icon: None,
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("lock".into()),
                        title: "Private by design".into(),
                        detail: "Your data is end-to-end encrypted and never leaves your control."
                            .into(),
                    },
                    InfoItem {
                        icon: Some("devices".into()),
                        title: "Multi-device".into(),
                        detail: "Use Vauchi on all your devices with seamless sync.".into(),
                    },
                ],
                a11y: Some(A11y {
                    label: Some("Welcome to Vauchi".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: "create_new".into(),
                    label: "Create new identity".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "have_identity".into(),
                    label: "I already have an identity".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_link_choice(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "link_choice".into(),
            title: "Restore your identity".into(),
            subtitle: Some("Choose how to restore your existing identity.".into()),
            components: vec![Component::InfoPanel {
                id: "link_choice_info".into(),
                icon: None,
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("devices".into()),
                        title: "Transfer from another device".into(),
                        detail: "Move all contacts and data from your old device via QR code."
                            .into(),
                    },
                    InfoItem {
                        icon: Some("link".into()),
                        title: "Link from another device".into(),
                        detail: "Scan a QR code on your other device to link this one.".into(),
                    },
                    InfoItem {
                        icon: Some("backup".into()),
                        title: "Restore from backup".into(),
                        detail: "Import identity only. Contacts will need to be re-established."
                            .into(),
                    },
                ],
                a11y: Some(A11y {
                    label: Some("Restore your identity".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: "transfer_device".into(),
                    label: "Transfer from another device".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "link_device".into(),
                    label: "Link from another device".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "restore_backup".into(),
                    label: "Restore from backup".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    /// Password-entry screen for the backup-restore flow (ADR-031,
    /// Phase 2B of `2026-05-03-core-file-picker-command`). Reached from
    /// `LinkChoice` after the user picks a file via
    /// `Command::FilePickFromUser{purpose:ImportBackup}`.
    fn build_backup_password_entry(&self) -> ScreenModel {
        let password_filled = !self.pending_backup_password.is_empty();
        ScreenModel {
            screen_id: "backup_password_entry".into(),
            title: "Enter backup password".into(),
            subtitle: Some("The password you set when you exported the backup.".into()),
            components: vec![Component::TextInput {
                id: "backup_password".into(),
                label: "Password".into(),
                value: String::new(),
                placeholder: Some("Backup password".into()),
                max_length: None,
                validation_error: None,
                input_type: InputType::Password,
                a11y: None,
                info_key: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "submit_backup_password".into(),
                    label: "Restore".into(),
                    style: ActionStyle::Primary,
                    enabled: password_filled,
                    a11y: None,
                },
                ScreenAction {
                    id: "back".into(),
                    label: "Back".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(2),
            ..Default::default()
        }
    }

    fn build_default_name(&self) -> ScreenModel {
        let name_filled = !self.data.display_name.trim().is_empty();
        ScreenModel {
            screen_id: "default_name".into(),
            title: "What's your name?".into(),
            subtitle: Some("How you appear to contacts".into()),
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
                    a11y: Some(A11y {
                        label: Some("Display name input".into()),
                        hint: Some("Enter the name others will see on your contact card".into()),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
            actions: vec![ScreenAction {
                id: "continue".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: name_filled,
                a11y: None,
            }],
            progress: self.progress(1),
            ..Default::default()
        }
    }

    fn build_groups_setup(&self) -> ScreenModel {
        let groups_info_key = if self.show_help_icons {
            Some("groups_purpose".into())
        } else {
            None
        };
        let items = self
            .data
            .selected_groups
            .iter()
            .map(|g| ToggleItem {
                id: g.name.clone(),
                label: g.name.clone(),
                selected: g.selected,
                subtitle: g.name_override.clone(),
                a11y: Some(A11y {
                    label: Some(format!(
                        "{}, {}",
                        g.name,
                        if g.selected {
                            "selected"
                        } else {
                            "not selected"
                        }
                    )),
                    hint: Some("Double tap to toggle".into()),
                    role: Some(AccessibilityRole::Toggle),
                }),
                info_key: groups_info_key.clone(),
            })
            .collect();

        ScreenModel {
            screen_id: "groups_setup".into(),
            title: "Choose your groups".into(),
            subtitle: Some("Organize who sees what".into()),
            components: vec![
                Component::Text {
                    id: "groups_recommendation".into(),
                    content: "Groups are optional, but we strongly recommend them. \
                              They let you control exactly who sees what on your card."
                        .into(),
                    style: TextStyle::Body,
                },
                Component::ToggleList {
                    id: "groups".into(),
                    label: "Suggested groups".into(),
                    items,
                    a11y: Some(A11y {
                        label: Some("Suggested groups options".into()),
                        hint: Some("Select items to include".into()),
                        role: None,
                    }),
                },
                Component::TextInput {
                    id: "custom_group".into(),
                    label: "Add a custom group".into(),
                    value: self.custom_group_input.clone(),
                    placeholder: Some("Type a group name and press Enter".into()),
                    max_length: Some(50),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some("Custom group name input".into()),
                        hint: Some("Enter a name for a custom contact group".into()),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
            actions: vec![
                // Commit the pending `custom_group` TextInput on the
                // current screen, keeping the user on groups_setup so
                // they can see the entry appear and optionally add
                // another. Wired to the existing `submit_custom_group`
                // handler at `handle_groups_setup`. Resolves
                // `_private/docs/problems/2026-04-20-onboarding-custom-group-add-invisible`.
                ScreenAction {
                    id: "submit_custom_group".into(),
                    label: "Add group".into(),
                    style: ActionStyle::Secondary,
                    enabled: !self.custom_group_input.trim().is_empty(),
                    a11y: None,
                },
                ScreenAction {
                    id: "continue".into(),
                    label: "Continue".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "skip".into(),
                    label: "Skip".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(2),
            ..Default::default()
        }
    }

    fn build_contact_info(&self) -> ScreenModel {
        let contact_info_key = if self.show_help_icons {
            Some("contact_info_optional".into())
        } else {
            None
        };
        let mut components: Vec<Component> = Vec::new();

        if self.phone_input_visible {
            components.push(Component::TextInput {
                id: "phone_input".into(),
                label: "Phone number".into(),
                value: self.phone_value.clone(),
                placeholder: Some("+1 555 123 4567".into()),
                max_length: Some(30),
                validation_error: None,
                input_type: InputType::Phone,
                a11y: Some(A11y {
                    label: Some("Phone number input".into()),
                    hint: Some("Enter your phone number".into()),
                    role: None,
                }),
                info_key: contact_info_key.clone(),
            });
        }

        if self.email_input_visible {
            components.push(Component::TextInput {
                id: "email_input".into(),
                label: "Email address".into(),
                value: self.email_value.clone(),
                placeholder: Some("you@example.com".into()),
                max_length: Some(254),
                validation_error: None,
                input_type: InputType::Email,
                a11y: Some(A11y {
                    label: Some("Email address input".into()),
                    hint: Some("Enter your email address".into()),
                    role: None,
                }),
                info_key: contact_info_key,
            });
        }

        for field in &self.data.fields {
            components.push(Component::Text {
                id: format!("social_{}", field.label.to_lowercase()),
                content: format!("{}: {}", field.label, field.value),
                style: TextStyle::Body,
            });
        }

        let mut actions = Vec::new();
        if !self.phone_input_visible {
            actions.push(ScreenAction {
                id: "show_phone".into(),
                label: "Add phone number".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        }
        if !self.email_input_visible {
            actions.push(ScreenAction {
                id: "show_email".into(),
                label: "Add email address".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            });
        }
        actions.push(ScreenAction {
            id: "add_social".into(),
            label: "Add social profile".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });
        actions.push(ScreenAction {
            id: "continue".into(),
            label: "Continue".into(),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: None,
        });
        actions.push(ScreenAction {
            id: "skip".into(),
            label: "Skip".into(),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: None,
        });

        ScreenModel {
            screen_id: "contact_info".into(),
            title: "Add contact info".into(),
            subtitle: Some("Optional \u{2014} you can add more later.".into()),
            components,
            actions,
            progress: self.progress(3),
            ..Default::default()
        }
    }

    fn build_what_next(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "what_next".into(),
            title: "What would you like to do?".into(),
            subtitle: Some("This is what contacts will see".into()),
            components: vec![],
            actions: vec![
                ScreenAction {
                    id: "exchange".into(),
                    label: "Exchange cards".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "import_contacts".into(),
                    label: "Import existing contacts".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "read_security".into(),
                    label: "Read about security".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "read_backup".into(),
                    label: "Read about backup".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "start_app".into(),
                    label: "Start using the app".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(4),
            ..Default::default()
        }
    }

    // ── Action handlers ─────────────────────────────────────────────

    fn navigate_to(&mut self, step: Step) -> ActionResult {
        self.step = step;
        ActionResult::NavigateTo(self.current_screen())
    }

    fn handle_identity_check(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "have_identity" => {
                self.navigate_to(Step::LinkChoice)
            }
            UserAction::ActionPressed { action_id } if action_id == "create_new" => {
                self.navigate_to(Step::DefaultName)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_link_choice(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "transfer_device" => {
                ActionResult::StartDeviceLink
            }
            UserAction::ActionPressed { action_id } if action_id == "link_device" => {
                ActionResult::StartDeviceLink
            }
            UserAction::ActionPressed { action_id } if action_id == "restore_backup" => {
                // ADR-031 file-picker: core drives the native picker
                // dialog instead of returning a chrome hint.
                // `AppEngine::handle_file_picked` (Onboarding arm)
                // routes the picked bytes back into this engine via
                // `set_pending_backup_bytes`, which transitions the
                // wizard to `Step::BackupPasswordEntry`. Phase 2B of
                // `2026-05-03-core-file-picker-command`.
                ActionResult::Commands {
                    commands: vec![Command::FilePickFromUser {
                        accepted_mime_types: backup_mime_types(),
                        purpose: FilePickPurpose::ImportBackup,
                    }],
                }
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                self.navigate_to(Step::IdentityCheck)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    /// Handles user input on the `BackupPasswordEntry` step.
    ///
    /// - `TextChanged{component_id:"backup_password"}` updates the
    ///   pending password so `Restore` enables once the field is non-empty.
    /// - `submit_backup_password` returns `ActionResult::Complete` —
    ///   the AppEngine completion path detects the step and calls
    ///   `Vauchi::import_full_backup(hex(bytes), password)`.
    /// - `back` clears pending bytes + password and returns to LinkChoice.
    fn handle_backup_password_entry(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "backup_password" => {
                self.pending_backup_password = value.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "submit_backup_password" => {
                if self.pending_backup_password.is_empty() || self.pending_backup_bytes.is_none() {
                    return ActionResult::ValidationError {
                        component_id: "backup_password".into(),
                        message: "Enter the backup password.".into(),
                    };
                }
                // AppEngine completion routing reads `current_step()` and
                // calls `take_pending_backup()` + `import_full_backup`.
                ActionResult::Complete
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                self.pending_backup_bytes = None;
                self.pending_backup_password.clear();
                self.navigate_to(Step::LinkChoice)
            }
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
            UserAction::ActionPressed { action_id }
                if action_id == "continue" || action_id == "submit_display_name" =>
            {
                if self.data.display_name.trim().is_empty() {
                    ActionResult::ValidationError {
                        component_id: "display_name".into(),
                        message: "Please enter your name.".into(),
                    }
                } else {
                    self.navigate_to(Step::GroupsSetup)
                }
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
            } if component_id == "custom_group" => {
                self.custom_group_input = value.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id.starts_with("group_name_override_") => {
                let Some(id) = component_id.strip_prefix("group_name_override_") else {
                    return ActionResult::UpdateScreen(self.current_screen());
                };
                if let Some(group) = self.data.selected_groups.iter_mut().find(|g| g.name == id) {
                    group.name_override = if value.trim().is_empty() {
                        None
                    } else {
                        Some(value.clone())
                    };
                }
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "submit_custom_group" => {
                let name = self.custom_group_input.trim().to_string();
                if !name.is_empty()
                    && !self
                        .data
                        .selected_groups
                        .iter()
                        .any(|g| g.name.eq_ignore_ascii_case(&name))
                {
                    self.data.selected_groups.push(GroupSetup {
                        name,
                        selected: true,
                        name_override: None,
                    });
                }
                self.custom_group_input.clear();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id }
                if action_id == "continue" || action_id == "skip" =>
            {
                // Auto-add any pending custom group text before advancing
                let pending = self.custom_group_input.trim().to_string();
                if !pending.is_empty()
                    && !self
                        .data
                        .selected_groups
                        .iter()
                        .any(|g| g.name.eq_ignore_ascii_case(&pending))
                {
                    self.data.selected_groups.push(GroupSetup {
                        name: pending,
                        selected: true,
                        name_override: None,
                    });
                    self.custom_group_input.clear();
                }
                self.navigate_to(Step::ContactInfo)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    fn handle_contact_info(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "show_phone" => {
                self.phone_input_visible = true;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "show_email" => {
                self.email_input_visible = true;
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "phone_input" => {
                self.phone_value = value.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::TextChanged {
                component_id,
                value,
            } if component_id == "email_input" => {
                self.email_value = value.clone();
                ActionResult::UpdateScreen(self.current_screen())
            }
            UserAction::ActionPressed { action_id } if action_id == "continue" => {
                self.sync_quick_add_fields();
                self.navigate_to(Step::WhatNext)
            }
            UserAction::ActionPressed { action_id } if action_id == "skip" => {
                self.navigate_to(Step::WhatNext)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    /// Sync non-empty phone/email values to OnboardingData.fields.
    fn sync_quick_add_fields(&mut self) {
        if !self.phone_value.trim().is_empty() {
            self.data.fields.push(FieldSetup {
                field_type: "phone".into(),
                label: "Phone".into(),
                value: self.phone_value.trim().to_string(),
                visible_to_groups: Vec::new(),
                shown: true,
            });
        }
        if !self.email_value.trim().is_empty() {
            self.data.fields.push(FieldSetup {
                field_type: "email".into(),
                label: "Email".into(),
                value: self.email_value.trim().to_string(),
                visible_to_groups: Vec::new(),
                shown: true,
            });
        }
    }

    fn handle_what_next(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => {
                let destination = match action_id.as_str() {
                    "exchange" => PostOnboardingDestination::Exchange,
                    "import_contacts" => PostOnboardingDestination::ImportContacts,
                    "read_security" => PostOnboardingDestination::SecurityInfo,
                    "read_backup" => PostOnboardingDestination::BackupSetup,
                    "start_app" => PostOnboardingDestination::MainScreen,
                    _ => return ActionResult::UpdateScreen(self.current_screen()),
                };
                ActionResult::CompleteWith { destination }
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}

impl OnboardingEngine {
    /// Add a field from external source (e.g., FormDialog save during onboarding).
    pub fn push_field(&mut self, field: FieldSetup) {
        self.data.fields.push(field);
    }

    /// Access onboarding data (groups, fields, name) for persistence at completion.
    pub fn onboarding_data(&self) -> &OnboardingData {
        &self.data
    }
}

impl WorkflowEngine for OnboardingEngine {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            Step::IdentityCheck => self.build_identity_check(),
            Step::LinkChoice => self.build_link_choice(),
            Step::BackupPasswordEntry => self.build_backup_password_entry(),
            Step::DefaultName => self.build_default_name(),
            Step::GroupsSetup => self.build_groups_setup(),
            Step::ContactInfo => self.build_contact_info(),
            Step::WhatNext => self.build_what_next(),
            _ => self.build_identity_check(),
        }
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match self.step {
            Step::IdentityCheck => self.handle_identity_check(&action),
            Step::LinkChoice => self.handle_link_choice(&action),
            Step::BackupPasswordEntry => self.handle_backup_password_entry(&action),
            Step::DefaultName => self.handle_default_name(&action),
            Step::GroupsSetup => self.handle_groups_setup(&action),
            Step::ContactInfo => self.handle_contact_info(&action),
            Step::WhatNext => self.handle_what_next(&action),
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
