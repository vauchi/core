// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The pre-gate onboarding screens: the welcome identity check and the
//! two "I already have an identity" paths it opens (device-link
//! instructions, backup password entry).

use super::*;

/// `accepted_mime_types` for the encrypted backup file picker. Frontends
/// may default to a coarser superset on platforms where the OS picker
/// doesn't filter by MIME (older Android variants).
fn backup_mime_types() -> Vec<String> {
    vec!["application/octet-stream".into(), "text/plain".into()]
}

impl OnboardingEngine {
    pub(super) fn build_identity_check(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "identity_check".into(),
            title: self.t("onboarding.welcome_title"),
            subtitle: Some(self.t("onboarding.welcome_subtitle")),
            components: vec![
                Component::InfoPanel {
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
                },
                self.have_identity_choices(),
            ],
            contextual_actions: vec![ScreenAction {
                id: "create_new".into(),
                label: self.t("onboarding.create_identity"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("onboarding.create_identity"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn have_identity_choices(&self) -> Component {
        Component::SectionedActionList {
            id: HAVE_IDENTITY_CHOICES.into(),
            sections: vec![Section {
                id: HAVE_IDENTITY_SECTION.into(),
                label: self.t("onboarding.have_identity"),
                items: vec![
                    self.entry_point(
                        "link_device",
                        "devices",
                        "onboarding.link_device_instructions_title",
                        "onboarding.link_device_instructions_subtitle",
                    ),
                    self.entry_point(
                        "load_backup",
                        "lock",
                        "onboarding.restore_backup_title",
                        "onboarding.backup_password_subtitle",
                    ),
                ],
            }],
        }
    }

    fn entry_point(
        &self,
        id: &str,
        icon: &str,
        label_key: &str,
        detail_key: &str,
    ) -> ActionListItem {
        let label = self.t(label_key);
        ActionListItem {
            id: id.into(),
            a11y: Some(A11y::labeled(label.clone())),
            label,
            icon: Some(icon.into()),
            detail: Some(self.t(detail_key)),
            info_key: None,
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
    pub(super) fn build_device_link_instructions(&self) -> ScreenModel {
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
    pub(super) fn build_backup_password_entry(&self) -> ScreenModel {
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

    pub(super) fn handle_identity_check(&mut self, action: &UserAction) -> ActionResult {
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
                accepted_extensions: FilePickPurpose::ImportBackup.accepted_extensions(),
                purpose: FilePickPurpose::ImportBackup,
            }],
        }
    }

    pub(super) fn handle_device_link_instructions(&mut self, action: &UserAction) -> ActionResult {
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
    pub(super) fn handle_backup_password_entry(&mut self, action: &UserAction) -> ActionResult {
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
}
