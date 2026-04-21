// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR / privacy engine — data export, deletion, and consent management.

use crate::ui::*;

/// Summary of what will be deleted, shown on the confirmation screen.
#[derive(Clone, Debug, Default)]
pub struct DeletionSummary {
    pub contact_count: usize,
    pub has_backup: bool,
    pub device_count: usize,
}

/// Engine that manages privacy and data settings (GDPR).
#[derive(Clone, Debug)]
pub struct GdprEngine {
    step: GdprStep,
    deletion_status: Option<String>,
    consent_summary: String,
    deletion_summary: DeletionSummary,
    /// Tracks which action triggered completion ("export" or "delete").
    last_action: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum GdprStep {
    /// Main privacy settings screen.
    Overview,
    /// Deletion confirmation screen showing what will be deleted.
    ConfirmDelete,
}

impl GdprEngine {
    pub fn new(deletion_status: Option<String>, consent_summary: String) -> Self {
        Self {
            step: GdprStep::Overview,
            deletion_status,
            consent_summary,
            deletion_summary: DeletionSummary::default(),
            last_action: None,
        }
    }

    /// Set the deletion summary data (contact count, backup status, device count).
    /// Called by AppEngine when constructing the screen with real data.
    pub fn with_deletion_summary(mut self, summary: DeletionSummary) -> Self {
        self.deletion_summary = summary;
        self
    }

    fn build_overview(&self) -> ScreenModel {
        let deletion_detail = self
            .deletion_status
            .clone()
            .unwrap_or_else(|| "No deletion requested".into());

        ScreenModel {
            screen_id: "privacy_settings".into(),
            title: "Privacy & Data".into(),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "privacy_info".into(),
                    icon: Some("privacy".into()),
                    title: "Data Status".into(),
                    items: vec![
                        InfoItem {
                            icon: None,
                            title: "Deletion Status".into(),
                            detail: deletion_detail,
                        },
                        InfoItem {
                            icon: None,
                            title: "Consent".into(),
                            detail: self.consent_summary.clone(),
                        },
                    ],
                    a11y: None,
                },
                Component::ActionList {
                    id: "consent_actions".into(),
                    items: vec![
                        ActionListItem {
                            id: "view_data".into(),
                            label: "View My Data".into(),
                            icon: Some("data".into()),
                            detail: Some("See what data is stored locally".into()),
                            a11y: None,
                            info_key: None,
                        },
                        ActionListItem {
                            id: "manage_consent".into(),
                            label: "Manage Consent".into(),
                            icon: Some("consent".into()),
                            detail: Some("Review and update data consent".into()),
                            a11y: None,
                            info_key: None,
                        },
                    ],
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "export".into(),
                    label: "Export Data".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "delete".into(),
                    label: "Delete Identity".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_delete(&self) -> ScreenModel {
        let s = &self.deletion_summary;
        let mut items = vec![
            InfoItem {
                icon: Some("identity".into()),
                title: "Your identity".into(),
                detail: "Cryptographic keys and display name — permanently destroyed".into(),
            },
            InfoItem {
                icon: Some("contacts".into()),
                title: format!("{} contact(s)", s.contact_count),
                detail: "All contact cards and exchange history — permanently deleted".into(),
            },
            InfoItem {
                icon: Some("cloud".into()),
                title: "Relay data".into(),
                detail: "Revocation broadcast sent to contacts, relay blobs purged".into(),
            },
            InfoItem {
                icon: Some("key".into()),
                title: "Keychain entry".into(),
                detail: "Device keystore/keychain entry — removed".into(),
            },
        ];

        if s.device_count > 1 {
            items.push(InfoItem {
                icon: Some("devices".into()),
                title: format!("{} linked device(s)", s.device_count - 1),
                detail: "Other devices will lose access to this identity".into(),
            });
        }

        items.push(InfoItem {
            icon: Some("warning".into()),
            title: "This cannot be undone".into(),
            detail: "After a 7-day grace period, all data is permanently destroyed. \
                     You can cancel during the grace period."
                .into(),
        });

        ScreenModel {
            screen_id: "delete_identity_summary".into(),
            title: "Delete Identity".into(),
            subtitle: Some("Review what will be deleted".into()),
            components: vec![Component::InfoPanel {
                id: "deletion_summary".into(),
                icon: Some("warning".into()),
                title: "The following will be deleted".into(),
                items,
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "confirm_delete".into(),
                    label: "Delete Identity".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    // Screen-reader context for the most irreversible
                    // action in the app. VoiceOver / TalkBack announce
                    // the explicit label rather than the shorter visible
                    // "Delete Identity" button text, then read the hint
                    // as the usage guidance.
                    a11y: Some(A11y {
                        label: Some("Delete identity permanently".into()),
                        hint: Some(
                            "Starts a 7-day grace period after which all your data is destroyed. \
                             Cannot be undone once the grace period ends."
                                .into(),
                        ),
                        role: None,
                    }),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            GdprStep::Overview => self.build_overview(),
            GdprStep::ConfirmDelete => self.build_confirm_delete(),
        }
    }
}

impl WorkflowEngine for GdprEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Overview: export triggers completion
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "export" =>
            {
                self.last_action = Some("export".into());
                ActionResult::Complete
            }
            // Overview: delete navigates to confirmation screen
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "delete" =>
            {
                self.step = GdprStep::ConfirmDelete;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Confirmation: confirm triggers deletion
            (GdprStep::ConfirmDelete, UserAction::ActionPressed { action_id })
                if action_id == "confirm_delete" =>
            {
                self.last_action = Some("delete".into());
                ActionResult::Complete
            }
            // Confirmation: cancel goes back to overview
            (GdprStep::ConfirmDelete, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                self.step = GdprStep::Overview;
                ActionResult::NavigateTo(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn collected_input(&self) -> Option<String> {
        self.last_action.clone()
    }
}

// INLINE_TEST_REQUIRED: Tests access private GdprStep enum and internal engine state
#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> GdprEngine {
        GdprEngine::new(None, "Active".into()).with_deletion_summary(DeletionSummary {
            contact_count: 5,
            has_backup: true,
            device_count: 2,
        })
    }

    #[test]
    fn starts_on_overview() {
        let e = engine();
        assert_eq!(e.current_screen().screen_id, "privacy_settings");
    }

    #[test]
    fn delete_navigates_to_summary() {
        let mut e = engine();
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "delete".into(),
        });
        match result {
            ActionResult::NavigateTo(screen) => {
                assert_eq!(screen.screen_id, "delete_identity_summary");
            }
            other => panic!("Expected NavigateTo, got {other:?}"),
        }
    }

    #[test]
    fn summary_shows_contact_count() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let screen = e.current_screen();
        let items = match &screen.components[0] {
            Component::InfoPanel { items, .. } => items,
            _ => panic!("Expected InfoPanel"),
        };
        assert!(
            items.iter().any(|i| i.title.contains("5 contact(s)")),
            "Should show contact count in summary"
        );
    }

    #[test]
    fn summary_shows_linked_devices() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let screen = e.current_screen();
        let items = match &screen.components[0] {
            Component::InfoPanel { items, .. } => items,
            _ => panic!("Expected InfoPanel"),
        };
        assert!(
            items.iter().any(|i| i.title.contains("1 linked device")),
            "Should show linked device count"
        );
    }

    #[test]
    fn summary_hides_devices_when_single() {
        let mut e = GdprEngine::new(None, "Active".into()).with_deletion_summary(DeletionSummary {
            contact_count: 0,
            has_backup: false,
            device_count: 1,
        });
        e.step = GdprStep::ConfirmDelete;
        let screen = e.current_screen();
        let items = match &screen.components[0] {
            Component::InfoPanel { items, .. } => items,
            _ => panic!("Expected InfoPanel"),
        };
        assert!(
            !items.iter().any(|i| i.title.contains("linked device")),
            "Should not show linked devices when only one device"
        );
    }

    #[test]
    fn confirm_delete_completes_with_delete_action() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm_delete".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(e.collected_input().as_deref(), Some("delete"));
    }

    #[test]
    fn cancel_returns_to_overview() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        match result {
            ActionResult::NavigateTo(screen) => {
                assert_eq!(screen.screen_id, "privacy_settings");
            }
            other => panic!("Expected NavigateTo, got {other:?}"),
        }
    }

    #[test]
    fn export_completes_from_overview() {
        let mut e = engine();
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "export".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(e.collected_input().as_deref(), Some("export"));
    }

    #[test]
    fn summary_has_grace_period_warning() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let screen = e.current_screen();
        let items = match &screen.components[0] {
            Component::InfoPanel { items, .. } => items,
            _ => panic!("Expected InfoPanel"),
        };
        assert!(
            items
                .iter()
                .any(|i| i.detail.contains("7-day grace period")),
            "Should mention grace period"
        );
    }
}
