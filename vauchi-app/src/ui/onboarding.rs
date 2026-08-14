// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Onboarding workflow engine — a pure state machine for the 6-step
//! onboarding flow (2 pre-gate + 4 main).  No Storage or Vauchi
//! dependency; the caller persists results when [`ActionResult::Complete`]
//! is returned.
//! Copy resolves through `i18n::get_string` in the locale set via
//! [`OnboardingEngine::with_locale`] (M3 S4a of
//! `2026-07-03-core-screens-bypass-i18n`); keys are the live-copy flat
//! `onboarding.*` family + shared `action.*` / `field_type.*`.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use vauchi_core::contact::labels::SUGGESTED_LABELS;
use vauchi_core::contact_card::{FieldType, validate_value};
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
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OnboardingData {
    pub display_name: String,
    pub selected_groups: Vec<GroupSetup>,
    pub fields: Vec<FieldSetup>,
}

/// A group the user can toggle during onboarding.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GroupSetup {
    pub name: String,
    pub selected: bool,
    pub name_override: Option<String>,
}

/// A contact field configured during onboarding.
#[cfg_attr(feature = "schema-gen", derive(schemars::JsonSchema))]
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    locale: Locale,
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
            locale: Locale::English,
        }
    }

    /// Sets the render locale (defaults to English). Threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory.
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
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
            title: self.t("onboarding.welcome_title"),
            subtitle: Some(self.t("onboarding.welcome_subtitle")),
            components: vec![Component::InfoPanel {
                id: "identity_check_info".into(),
                icon: None,
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("lock".into()),
                        title: self.t("onboarding.welcome_private_title"),
                        detail: self.t("onboarding.welcome_private_desc"),
                    },
                    InfoItem {
                        icon: Some("devices".into()),
                        title: self.t("onboarding.welcome_multidevice_title"),
                        detail: self.t("onboarding.welcome_multidevice_desc"),
                    },
                ],
                a11y: Some(A11y {
                    label: Some(self.t("onboarding.welcome_title")),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "create_new".into(),
                    label: self.t("onboarding.create_identity"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.create_identity"))),
                },
                ScreenAction {
                    id: "link_device".into(),
                    label: self.t("onboarding.restore_link_title"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.restore_link_title"))),
                },
                ScreenAction {
                    id: "load_backup".into(),
                    label: self.t("onboarding.restore_backup_title"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.restore_backup_title"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    /// Pre-gate instruction screen for the "link this device" path.
    ///
    /// The user reaches this from `IdentityCheck` by tapping "Link this
    /// device". Core is humble: the screen only explains how to get the
    /// invitation (open the link from the other device or scan its QR code)
    /// and offers a scan button that emits `Command::QrRequestScan`. The
    /// actual invitation ingestion happens via the existing `LinkOpened`
    /// deep-link path and the `Event::QrScanned` hardware-event path in
    /// `AppEngine`, both of which route to `AppScreen::DeviceLinkJoin`.
    fn build_device_link_instructions(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "device_link_instructions".into(),
            title: self.t("onboarding.link_device_instructions_title"),
            subtitle: Some(self.t("onboarding.link_device_instructions_subtitle")),
            components: vec![Component::InfoPanel {
                id: "device_link_instructions_info".into(),
                icon: Some("devices".into()),
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("qr".into()),
                        title: self.t("onboarding.link_device_instructions_scan"),
                        detail: self.t("onboarding.link_device_instructions_scan_desc"),
                    },
                    InfoItem {
                        icon: Some("link".into()),
                        title: self.t("onboarding.link_device_instructions_link"),
                        detail: self.t("onboarding.link_device_instructions_link_desc"),
                    },
                ],
                a11y: Some(A11y {
                    label: Some(self.t("onboarding.link_device_instructions_title")),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "scan_qr".into(),
                    label: self.t("qr.scan_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("qr.scan_button"))),
                },
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    /// Password-entry screen for the backup-restore flow (ADR-031,
    /// Phase 2B of `2026-05-03-core-file-picker-command`). Reached from
    /// `IdentityCheck` after the user picks a file via
    /// `Command::FilePickFromUser{purpose:ImportBackup}`.
    fn build_backup_password_entry(&self) -> ScreenModel {
        let password_filled = !self.pending_backup_password.is_empty();
        ScreenModel {
            screen_id: "backup_password_entry".into(),
            title: self.t("onboarding.backup_password_title"),
            subtitle: Some(self.t("onboarding.backup_password_subtitle")),
            components: vec![Component::TextInput {
                id: "backup_password".into(),
                label: self.t("backup.password"),
                value: String::new(),
                placeholder: Some(self.t("onboarding.backup_password_placeholder")),
                max_length: None,
                validation_error: None,
                input_type: InputType::Password,
                a11y: Some(A11y::labeled(self.t("backup.password"))),
                info_key: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "submit_backup_password".into(),
                    label: self.t("onboarding.restore_button"),
                    style: ActionStyle::Primary,
                    enabled: password_filled,
                    a11y: Some(A11y::labeled(self.t("onboarding.restore_button"))),
                },
                ScreenAction {
                    id: "back".into(),
                    label: self.t("action.back"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.back"))),
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
            title: self.t("onboarding.name_title"),
            subtitle: Some(self.t("onboarding.name_subtitle")),
            components: vec![
                Component::Text {
                    a11y: None,
                    id: "name_instruction".into(),
                    content: self.t("onboarding.name_instruction"),
                    style: TextStyle::Body,
                },
                Component::TextInput {
                    id: "display_name".into(),
                    label: self.t("onboarding.name_label"),
                    value: self.data.display_name.clone(),
                    placeholder: Some(self.t("onboarding.name_placeholder")),
                    max_length: Some(100),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("onboarding.name_a11y")),
                        hint: Some(self.t("onboarding.name_a11y_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
            contextual_actions: vec![ScreenAction {
                id: "continue".into(),
                label: self.t("action.continue"),
                style: ActionStyle::Primary,
                enabled: name_filled,
                a11y: Some(A11y::labeled(self.t("action.continue"))),
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
                            self.t("onboarding.a11y_selected")
                        } else {
                            self.t("onboarding.a11y_not_selected")
                        }
                    )),
                    hint: Some(self.t("onboarding.a11y_toggle_hint")),
                    role: Some(AccessibilityRole::Toggle),
                }),
                info_key: groups_info_key.clone(),
            })
            .collect();

        ScreenModel {
            screen_id: "groups_setup".into(),
            title: self.t("onboarding.groups_title"),
            subtitle: Some(self.t("onboarding.groups_subtitle")),
            components: vec![
                Component::Text {
                    a11y: None,
                    id: "groups_recommendation".into(),
                    content: self.t("onboarding.groups_recommendation"),
                    style: TextStyle::Body,
                },
                Component::ToggleList {
                    id: "groups".into(),
                    label: self.t("onboarding.groups_suggested"),
                    items,
                    a11y: Some(A11y {
                        label: Some(self.t("onboarding.groups_suggested_a11y")),
                        hint: Some(self.t("onboarding.groups_suggested_a11y_hint")),
                        role: None,
                    }),
                },
                Component::TextInput {
                    id: "custom_group".into(),
                    label: self.t("onboarding.groups_custom_label"),
                    value: self.custom_group_input.clone(),
                    placeholder: Some(self.t("onboarding.groups_custom_placeholder")),
                    max_length: Some(50),
                    validation_error: None,
                    input_type: InputType::Text,
                    a11y: Some(A11y {
                        label: Some(self.t("onboarding.groups_custom_a11y")),
                        hint: Some(self.t("onboarding.groups_custom_a11y_hint")),
                        role: None,
                    }),
                    info_key: None,
                },
            ],
            contextual_actions: vec![
                // Commit the pending `custom_group` TextInput on the
                // current screen, keeping the user on groups_setup so
                // they can see the entry appear and optionally add
                // another. Wired to the existing `submit_custom_group`
                // handler at `handle_groups_setup`. Resolves
                // `_private/docs/problems/2026-04-20-onboarding-custom-group-add-invisible`.
                ScreenAction {
                    id: "submit_custom_group".into(),
                    label: self.t("onboarding.groups_add"),
                    style: ActionStyle::Secondary,
                    enabled: !self.custom_group_input.trim().is_empty(),
                    a11y: Some(A11y::labeled(self.t("onboarding.groups_add"))),
                },
                ScreenAction {
                    id: "continue".into(),
                    label: self.t("action.continue"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.continue"))),
                },
                ScreenAction {
                    id: "skip".into(),
                    label: self.t("onboarding.skip"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.skip"))),
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
                label: self.t("onboarding.phone_placeholder"),
                value: self.phone_value.clone(),
                placeholder: Some("+1 555 123 4567".into()),
                max_length: Some(30),
                validation_error: None,
                input_type: InputType::Phone,
                a11y: Some(A11y {
                    label: Some(self.t("onboarding.phone_a11y")),
                    hint: Some(self.t("onboarding.phone_a11y_hint")),
                    role: None,
                }),
                info_key: contact_info_key.clone(),
            });
        }

        if self.email_input_visible {
            components.push(Component::TextInput {
                id: "email_input".into(),
                label: self.t("onboarding.email_placeholder"),
                value: self.email_value.clone(),
                placeholder: Some(self.t("onboarding.email_example")),
                max_length: Some(254),
                validation_error: None,
                input_type: InputType::Email,
                a11y: Some(A11y {
                    label: Some(self.t("onboarding.email_a11y")),
                    hint: Some(self.t("onboarding.email_a11y_hint")),
                    role: None,
                }),
                info_key: contact_info_key,
            });
        }

        for field in &self.data.fields {
            components.push(Component::Text {
                a11y: None,
                id: format!("social_{}", field.label.to_lowercase()),
                content: format!("{}: {}", field.label, field.value),
                style: TextStyle::Body,
            });
        }

        let mut actions = Vec::new();
        if !self.phone_input_visible {
            actions.push(ScreenAction {
                id: "show_phone".into(),
                label: self.t("onboarding.add_phone"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("onboarding.add_phone"))),
            });
        }
        if !self.email_input_visible {
            actions.push(ScreenAction {
                id: "show_email".into(),
                label: self.t("onboarding.add_email"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("onboarding.add_email"))),
            });
        }
        actions.push(ScreenAction {
            id: "add_social".into(),
            label: self.t("onboarding.add_social"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(self.t("onboarding.add_social"))),
        });
        actions.push(ScreenAction {
            id: "continue".into(),
            label: self.t("action.continue"),
            style: ActionStyle::Primary,
            enabled: true,
            a11y: Some(A11y::labeled(self.t("action.continue"))),
        });
        actions.push(ScreenAction {
            id: "skip".into(),
            label: self.t("onboarding.skip"),
            style: ActionStyle::Secondary,
            enabled: true,
            a11y: Some(A11y::labeled(self.t("onboarding.skip"))),
        });

        ScreenModel {
            screen_id: "contact_info".into(),
            title: self.t("onboarding.info_title"),
            subtitle: Some(self.t("onboarding.info_subtitle")),
            components,
            contextual_actions: actions,
            progress: self.progress(3),
            ..Default::default()
        }
    }

    fn build_what_next(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "what_next".into(),
            title: self.t("onboarding.done_title"),
            subtitle: Some(self.t("onboarding.done_subtitle")),
            components: vec![],
            contextual_actions: vec![
                // The final onboarding step's job is to land the user
                // in the app — "Start using the app" is the natural
                // exit, so it owns the primary affordance. Exchange
                // and Import remain as optional shortcuts for users
                // who know exactly what they want to do first.
                //
                // "Read about security" and "Read about backup" were
                // peers here until 2026-05-21; they came off this
                // screen because the docs reading list belonged in
                // Help, not on the onboarding finish line — the
                // 5-button flat menu hid which option actually
                // ended onboarding. See
                // _private/docs/problems/2026-05-21-mobile-onboarding-
                // final-step-and-skip-fold G2/G3.
                ScreenAction {
                    id: "start_app".into(),
                    label: self.t("onboarding.start_app"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.start_app"))),
                },
                ScreenAction {
                    id: "exchange".into(),
                    label: self.t("onboarding.exchange_cards"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.exchange_cards"))),
                },
                ScreenAction {
                    id: "import_contacts".into(),
                    label: self.t("onboarding.import_existing"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("onboarding.import_existing"))),
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
            UserAction::ActionPressed { action_id } if action_id == "create_new" => {
                self.navigate_to(Step::DefaultName)
            }
            UserAction::ActionPressed { action_id } if action_id == "link_device" => {
                // Transition to the instruction screen. The actual invitation
                // is ingested through the existing `LinkOpened` deep-link or
                // `Event::QrScanned` hardware path in `AppEngine`, both of
                // which route to `AppScreen::DeviceLinkJoin`. The scan button
                // on the instructions screen emits `Command::QrRequestScan`
                // directly, so no `StartDeviceLink` result is needed here
                // (`2026-07-06-mobile-domain-shell-violations` I9).
                self.navigate_to(Step::DeviceLinkInstructions)
            }
            UserAction::ActionPressed { action_id } if action_id == "load_backup" => {
                self.trigger_backup_restore()
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    /// ADR-031 file-picker: core drives the native picker dialog instead of
    /// returning a chrome hint. `AppEngine::handle_file_picked` (Onboarding
    /// arm) routes the picked bytes back into this engine via
    /// `set_pending_backup_bytes`, which transitions the wizard to
    /// `Step::BackupPasswordEntry`. Phase 2B of
    /// `2026-05-03-core-file-picker-command`.
    fn trigger_backup_restore(&self) -> ActionResult {
        ActionResult::Commands {
            commands: vec![Command::FilePickFromUser {
                accepted_mime_types: backup_mime_types(),
                purpose: FilePickPurpose::ImportBackup,
            }],
        }
    }

    fn handle_device_link_instructions(&mut self, action: &UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } if action_id == "scan_qr" => {
                ActionResult::Commands {
                    commands: vec![Command::QrRequestScan],
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
    /// - `back` clears pending bytes + password and returns to IdentityCheck.
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
                        message: self.t("onboarding.error_backup_password"),
                    };
                }
                // AppEngine completion routing reads `current_step()` and
                // calls `take_pending_backup()` + `import_full_backup`.
                ActionResult::Complete
            }
            UserAction::ActionPressed { action_id } if action_id == "back" => {
                self.pending_backup_bytes = None;
                self.pending_backup_password.clear();
                self.navigate_to(Step::IdentityCheck)
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
                        message: self.t("onboarding.error_name_required"),
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
                if let Some(error) = self.quick_add_validation_error() {
                    return error;
                }
                self.sync_quick_add_fields();
                self.navigate_to(Step::WhatNext)
            }
            UserAction::ActionPressed { action_id } if action_id == "skip" => {
                self.navigate_to(Step::WhatNext)
            }
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }

    /// First validation error among the visible quick-add inputs, phone
    /// before email.
    ///
    /// Invalid values must block the step here: `complete_onboarding`
    /// persists `data.fields` best-effort, so an invalid value collected
    /// now would be dropped silently at completion (exploratory
    /// verification 2026-08-07, TUI-3). Gated on the input being visible
    /// because the shell boundary is untrusted (DC-02): a value reported
    /// for a hidden input has no component to anchor the error on, so it
    /// is ignored rather than validated or collected.
    fn quick_add_validation_error(&self) -> Option<ActionResult> {
        if self.phone_input_visible {
            let phone = self.phone_value.trim();
            if !phone.is_empty() && validate_value(&FieldType::Phone, phone).is_err() {
                return Some(ActionResult::ValidationError {
                    component_id: "phone_input".into(),
                    message: self.t("validation.invalid_phone"),
                });
            }
        }
        if self.email_input_visible {
            let email = self.email_value.trim();
            if !email.is_empty() && validate_value(&FieldType::Email, email).is_err() {
                return Some(ActionResult::ValidationError {
                    component_id: "email_input".into(),
                    message: self.t("validation.invalid_email"),
                });
            }
        }
        None
    }

    /// Sync non-empty phone/email values to OnboardingData.fields.
    /// Hidden inputs are ignored per the visibility gate documented on
    /// `quick_add_validation_error`.
    fn sync_quick_add_fields(&mut self) {
        if self.phone_input_visible && !self.phone_value.trim().is_empty() {
            self.data.fields.push(FieldSetup {
                field_type: "phone".into(),
                label: self.t("field_type.phone"),
                value: self.phone_value.trim().to_string(),
                visible_to_groups: Vec::new(),
                shown: true,
            });
        }
        if self.email_input_visible && !self.email_value.trim().is_empty() {
            self.data.fields.push(FieldSetup {
                field_type: "email".into(),
                label: self.t("field_type.email"),
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
                    "start_app" => PostOnboardingDestination::MainScreen,
                    _ => return ActionResult::UpdateScreen(self.current_screen()),
                };
                // Signal onboarding completion with the chosen destination.
                // AppEngine routes this to identity creation + navigation so
                // frontends no longer enumerate onboarding screen ids to
                // detect completion (`2026-07-06-mobile-domain-shell-violations`
                // I7/A13).
                ActionResult::OnboardingComplete { destination }
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
    fn engine_output(&self) -> Option<EngineOutput> {
        Some(EngineOutput::Onboarding(Box::new(
            crate::ui::OnboardingSnapshot {
                data: self.data.clone(),
                step: self.step,
                pending_backup: self.pending_backup_bytes.as_ref().map(|bytes| {
                    crate::ui::PendingBackup {
                        bytes: bytes.clone(),
                        password: self.pending_backup_password.clone(),
                    }
                }),
            },
        )))
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        use crate::ui::OnboardingUpdate as U;
        let crate::ui::EngineUpdate::Onboarding(update) = update else {
            return false;
        };
        match update {
            U::PendingBackupBytes(bytes) => self.set_pending_backup_bytes(bytes),
            U::ClearPendingBackup => {
                let _ = self.take_pending_backup();
            }
            U::ResetToIdentityCheck => {
                // Re-emitting "back" from BackupPasswordEntry clears
                // pending bytes + password and routes to IdentityCheck.
                #[allow(clippy::let_underscore_must_use)]
                let _ = self.handle_action(UserAction::ActionPressed {
                    action_id: "back".into(),
                });
            }
            U::PushField(field) => self.push_field(field),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        match self.step {
            Step::IdentityCheck => self.build_identity_check(),
            Step::DeviceLinkInstructions => self.build_device_link_instructions(),
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
            Step::DeviceLinkInstructions => self.handle_device_link_instructions(&action),
            Step::BackupPasswordEntry => self.handle_backup_password_entry(&action),
            Step::DefaultName => self.handle_default_name(&action),
            Step::GroupsSetup => self.handle_groups_setup(&action),
            Step::ContactInfo => self.handle_contact_info(&action),
            Step::WhatNext => self.handle_what_next(&action),
            _ => ActionResult::UpdateScreen(self.current_screen()),
        }
    }
}
