// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR / privacy engine — data export, deletion, and consent management.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3b of `2026-07-03-core-screens-bypass-i18n`); keys
//! live in the `privacy.*` / `shred.panic_*` + shared `action.*` families.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Summary of what will be deleted, shown on the confirmation screen.
#[derive(Clone, Debug, Default)]
pub struct DeletionSummary {
    pub contact_count: usize,
    pub has_backup: bool,
    pub device_count: usize,
}

/// Current consent grant state per type, rendered as toggles on the
/// consent-management sub-screen. Item ids match `ConsentType::as_str()`
/// so the AppEngine intercept can `ConsentType::parse` them.
#[derive(Clone, Debug, Default)]
pub struct ConsentStatus {
    pub data_processing: bool,
    pub contact_sharing: bool,
    pub recovery_vouching: bool,
}

impl ConsentStatus {
    /// Build from a consent log, taking the latest decision per type.
    pub fn from_consent_records(records: &[vauchi_core::ConsentRecord]) -> Self {
        let granted = |t: vauchi_core::ConsentType| {
            records
                .iter()
                .rfind(|r| r.consent_type == t)
                .map(|r| r.granted)
                .unwrap_or(false)
        };
        Self {
            data_processing: granted(vauchi_core::ConsentType::DataProcessing),
            contact_sharing: granted(vauchi_core::ConsentType::ContactSharing),
            recovery_vouching: granted(vauchi_core::ConsentType::RecoveryVouching),
        }
    }
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
    /// Current consent grant state, rendered on the consent sub-screen.
    consent: ConsentStatus,
    /// Whether an identity deletion is currently scheduled (grace period
    /// active). Drives the cancel-vs-delete action on the overview.
    deletion_scheduled: bool,
    /// Whether a scheduled deletion's grace period has elapsed, so it can
    /// be executed now. Drives the "Delete Now" action.
    deletion_executable: bool,
    locale: Locale,
}

#[derive(Clone, Debug, PartialEq)]
enum GdprStep {
    /// Main privacy settings screen.
    Overview,
    /// Deletion confirmation screen showing what will be deleted.
    ConfirmDelete,
    /// Consent-management screen — per-type grant toggles.
    ManageConsent,
    /// Confirm executing a scheduled deletion now (grace elapsed).
    ConfirmExecute,
    /// Confirm an immediate emergency wipe (panic shred).
    ConfirmShred,
}

impl GdprEngine {
    pub fn new(deletion_status: Option<String>, consent_summary: String, locale: Locale) -> Self {
        Self {
            step: GdprStep::Overview,
            deletion_status,
            consent_summary,
            deletion_summary: DeletionSummary::default(),
            last_action: None,
            consent: ConsentStatus::default(),
            deletion_scheduled: false,
            deletion_executable: false,
            locale,
        }
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn t_count(&self, key: &str, count: usize) -> String {
        get_string_with_args(self.locale, key, &[("count", &count.to_string())])
    }

    /// Set the deletion summary data (contact count, backup status, device count).
    /// Called by AppEngine when constructing the screen with real data.
    pub fn with_deletion_summary(mut self, summary: DeletionSummary) -> Self {
        self.deletion_summary = summary;
        self
    }

    /// Set the current consent grant state, rendered on the consent
    /// sub-screen. Called by the AppEngine with live state.
    pub fn with_consent(mut self, consent: ConsentStatus) -> Self {
        self.consent = consent;
        self
    }

    /// Mark whether an identity deletion is scheduled, so the overview
    /// offers "Cancel Deletion" instead of "Delete Identity".
    pub fn with_deletion_scheduled(mut self, scheduled: bool) -> Self {
        self.deletion_scheduled = scheduled;
        self
    }

    /// Mark whether a scheduled deletion can be executed now (grace
    /// elapsed), so the overview offers "Delete Now".
    pub fn with_deletion_executable(mut self, executable: bool) -> Self {
        self.deletion_executable = executable;
        self
    }

    fn build_overview(&self) -> ScreenModel {
        let deletion_detail = self
            .deletion_status
            .clone()
            .unwrap_or_else(|| self.t("privacy.no_deletion_requested"));

        ScreenModel {
            screen_id: "privacy_settings".into(),
            title: self.t("privacy.title"),
            subtitle: None,
            components: vec![
                Component::InfoPanel {
                    id: "privacy_info".into(),
                    icon: Some("privacy".into()),
                    title: self.t("privacy.data_status"),
                    items: vec![
                        InfoItem {
                            icon: None,
                            title: self.t("privacy.deletion_status"),
                            detail: deletion_detail,
                        },
                        InfoItem {
                            icon: None,
                            title: self.t("privacy.consent"),
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
                            label: self.t("privacy.view_data"),
                            icon: Some("data".into()),
                            detail: Some(self.t("privacy.view_data_desc")),
                            a11y: Some(A11y::labeled(self.t("privacy.view_data"))),
                            info_key: None,
                        },
                        ActionListItem {
                            id: "manage_consent".into(),
                            label: self.t("privacy.manage_consent"),
                            icon: Some("consent".into()),
                            detail: Some(self.t("privacy.manage_consent_desc")),
                            a11y: Some(A11y::labeled(self.t("privacy.manage_consent"))),
                            info_key: None,
                        },
                    ],
                },
            ],
            actions: {
                let mut actions = vec![ScreenAction {
                    id: "export".into(),
                    label: self.t("privacy.export_data"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("privacy.export_data"))),
                }];
                if self.deletion_scheduled {
                    actions.push(ScreenAction {
                        id: "cancel_deletion".into(),
                        label: self.t("privacy.cancel_deletion"),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("privacy.cancel_deletion"))),
                    });
                    if self.deletion_executable {
                        actions.push(ScreenAction {
                            id: "execute_deletion".into(),
                            label: self.t("privacy.delete_now"),
                            style: ActionStyle::Destructive,
                            enabled: true,
                            a11y: Some(A11y::labeled(self.t("privacy.delete_now"))),
                        });
                    }
                } else {
                    actions.push(ScreenAction {
                        id: "delete".into(),
                        label: self.t("privacy.delete_identity"),
                        style: ActionStyle::Destructive,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("privacy.delete_identity"))),
                    });
                }
                actions.push(ScreenAction {
                    id: "panic_shred".into(),
                    label: self.t("shred.panic_title"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("shred.panic_title"))),
                });
                actions
            },
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_delete(&self) -> ScreenModel {
        let s = &self.deletion_summary;
        let mut items = vec![
            InfoItem {
                icon: Some("identity".into()),
                title: self.t("privacy.delete.identity_title"),
                detail: self.t("privacy.delete.identity_detail"),
            },
            InfoItem {
                icon: Some("contacts".into()),
                title: self.t_count("privacy.delete.contacts_title", s.contact_count),
                detail: self.t("privacy.delete.contacts_detail"),
            },
            InfoItem {
                icon: Some("cloud".into()),
                title: self.t("privacy.delete.relay_title"),
                detail: self.t("privacy.delete.relay_detail"),
            },
            InfoItem {
                icon: Some("key".into()),
                title: self.t("privacy.delete.keychain_title"),
                detail: self.t("privacy.delete.keychain_detail"),
            },
        ];

        if s.device_count > 1 {
            items.push(InfoItem {
                icon: Some("devices".into()),
                title: self.t_count("privacy.delete.devices_title", s.device_count - 1),
                detail: self.t("privacy.delete.devices_detail"),
            });
        }

        items.push(InfoItem {
            icon: Some("warning".into()),
            title: self.t("privacy.delete.irreversible_title"),
            detail: self.t("privacy.delete.irreversible_detail"),
        });

        ScreenModel {
            screen_id: "delete_identity_summary".into(),
            title: self.t("privacy.delete_identity"),
            subtitle: Some(self.t("privacy.delete.review_subtitle")),
            components: vec![Component::InfoPanel {
                id: "deletion_summary".into(),
                icon: Some("warning".into()),
                title: self.t("privacy.delete.list_title"),
                items,
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "confirm_delete".into(),
                    label: self.t("privacy.delete_identity"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    // Screen-reader context for the most irreversible
                    // action in the app. VoiceOver / TalkBack announce
                    // the explicit label rather than the shorter visible
                    // "Delete Identity" button text, then read the hint
                    // as the usage guidance.
                    a11y: Some(A11y {
                        label: Some(self.t("privacy.delete.a11y_confirm")),
                        hint: Some(self.t("privacy.delete.a11y_confirm_hint")),
                        role: None,
                    }),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_consent(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "manage_consent".into(),
            title: self.t("privacy.manage_consent"),
            subtitle: Some(self.t("privacy.manage_consent_desc")),
            components: vec![Component::SettingsGroup {
                id: "consent".into(),
                label: self.t("privacy.consent"),
                items: vec![
                    SettingsItem {
                        id: "data_processing".into(),
                        label: self.t("privacy.consent_data_processing"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.data_processing,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_data_processing"))),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "contact_sharing".into(),
                        label: self.t("privacy.consent_contact_sharing"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.contact_sharing,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_contact_sharing"))),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "recovery_vouching".into(),
                        label: self.t("privacy.consent_recovery_vouching"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.recovery_vouching,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_recovery_vouching"))),
                        info_key: None,
                    },
                ],
            }],
            actions: vec![ScreenAction {
                id: "cancel".into(),
                label: self.t("action.back"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.back"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_execute(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "confirm_execute_deletion".into(),
            title: self.t("privacy.delete_now"),
            subtitle: Some(self.t("privacy.delete_now.subtitle")),
            components: vec![Component::InfoPanel {
                id: "execute_warning".into(),
                icon: Some("warning".into()),
                title: self.t("privacy.delete_now.panel_title"),
                items: vec![InfoItem {
                    icon: Some("warning".into()),
                    title: self.t("privacy.delete.irreversible_title"),
                    detail: self.t("privacy.delete_now.detail"),
                }],
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "confirm_execute".into(),
                    label: self.t("privacy.delete_now.confirm"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("privacy.delete_now.confirm"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("privacy.delete_now.keep"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("privacy.delete_now.keep"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_shred(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "confirm_panic_shred".into(),
            title: self.t("shred.panic_title"),
            subtitle: Some(self.t("shred.panic_subtitle")),
            components: vec![Component::InfoPanel {
                id: "shred_warning".into(),
                icon: Some("warning".into()),
                title: self.t("shred.panic_panel_title"),
                items: vec![InfoItem {
                    icon: Some("warning".into()),
                    title: self.t("privacy.delete.irreversible_title"),
                    detail: self.t("shred.panic_detail"),
                }],
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "confirm_shred".into(),
                    label: self.t("shred.panic_confirm_button"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("shred.panic_confirm_button"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
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
            GdprStep::ManageConsent => self.build_consent(),
            GdprStep::ConfirmExecute => self.build_confirm_execute(),
            GdprStep::ConfirmShred => self.build_confirm_shred(),
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
            // Overview: cancel a scheduled deletion (routing performs it)
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "cancel_deletion" =>
            {
                self.last_action = Some("cancel_deletion".into());
                ActionResult::Complete
            }
            // Overview: delete navigates to confirmation screen
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "delete" =>
            {
                self.step = GdprStep::ConfirmDelete;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Overview: execute-now navigates to its confirmation screen
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "execute_deletion" =>
            {
                self.step = GdprStep::ConfirmExecute;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Overview: panic shred navigates to its confirmation screen
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "panic_shred" =>
            {
                self.step = GdprStep::ConfirmShred;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Confirmation: confirm triggers deletion
            (GdprStep::ConfirmDelete, UserAction::ActionPressed { action_id })
                if action_id == "confirm_delete" =>
            {
                self.last_action = Some("delete".into());
                ActionResult::Complete
            }
            // Execute confirmation: confirm triggers immediate execution
            (GdprStep::ConfirmExecute, UserAction::ActionPressed { action_id })
                if action_id == "confirm_execute" =>
            {
                self.last_action = Some("execute".into());
                ActionResult::Complete
            }
            // Shred confirmation: confirm triggers the emergency wipe
            (GdprStep::ConfirmShred, UserAction::ActionPressed { action_id })
                if action_id == "confirm_shred" =>
            {
                self.last_action = Some("shred".into());
                ActionResult::Complete
            }
            // Any confirmation screen: cancel goes back to overview
            (
                GdprStep::ConfirmDelete | GdprStep::ConfirmExecute | GdprStep::ConfirmShred,
                UserAction::ActionPressed { action_id },
            ) if action_id == "cancel" => {
                self.step = GdprStep::Overview;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Overview: selecting "manage_consent" opens the consent screen
            (GdprStep::Overview, UserAction::ListItemSelected { item_id, .. })
                if item_id == "manage_consent" =>
            {
                self.step = GdprStep::ManageConsent;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Consent screen: back to overview
            (GdprStep::ManageConsent, UserAction::ActionPressed { action_id })
                if action_id == "cancel" =>
            {
                self.step = GdprStep::Overview;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Consent screen: a toggle flips the in-memory display; the
            // AppEngine intercept (`persist_consent_toggle`) owns the
            // grant/revoke persistence, flipping the same starting state.
            (
                GdprStep::ManageConsent,
                UserAction::SettingsToggled {
                    component_id,
                    item_id,
                },
            ) if component_id == "consent" => {
                match item_id.as_str() {
                    "data_processing" => {
                        self.consent.data_processing = !self.consent.data_processing
                    }
                    "contact_sharing" => {
                        self.consent.contact_sharing = !self.consent.contact_sharing
                    }
                    "recovery_vouching" => {
                        self.consent.recovery_vouching = !self.consent.recovery_vouching
                    }
                    _ => {}
                }
                ActionResult::UpdateScreen(self.build_screen())
            }
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        use crate::ui::GdprChoice;
        let choice = match self.last_action.as_deref() {
            Some("export") => GdprChoice::Export,
            Some("delete") => GdprChoice::Delete,
            Some("cancel_deletion") => GdprChoice::CancelDeletion,
            Some("execute") => GdprChoice::Execute,
            Some("shred") => GdprChoice::Shred,
            _ => return None,
        };
        Some(crate::ui::EngineOutput::Gdpr(choice))
    }
}

// INLINE_TEST_REQUIRED: Tests access private GdprStep enum and internal engine state
#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> GdprEngine {
        GdprEngine::new(None, "Active".into(), Locale::English).with_deletion_summary(
            DeletionSummary {
                contact_count: 5,
                has_backup: true,
                device_count: 2,
            },
        )
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
        let mut e = GdprEngine::new(None, "Active".into(), Locale::English).with_deletion_summary(
            DeletionSummary {
                contact_count: 0,
                has_backup: false,
                device_count: 1,
            },
        );
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
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(crate::ui::GdprChoice::Delete))
        );
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
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(crate::ui::GdprChoice::Export))
        );
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

    // @internal
    #[test]
    fn manage_consent_navigates_to_consent_screen() {
        let mut e = engine();
        let result = e.handle_action(UserAction::ListItemSelected {
            component_id: "consent_actions".into(),
            item_id: "manage_consent".into(),
        });
        match result {
            ActionResult::NavigateTo(screen) => {
                assert_eq!(screen.screen_id, "manage_consent");
            }
            other => panic!("Expected NavigateTo, got {other:?}"),
        }
    }

    // @internal
    #[test]
    fn consent_screen_reflects_grant_state() {
        let mut e =
            GdprEngine::new(None, "Active".into(), Locale::English).with_consent(ConsentStatus {
                data_processing: true,
                contact_sharing: false,
                recovery_vouching: true,
            });
        e.step = GdprStep::ManageConsent;
        let screen = e.current_screen();
        let items = match &screen.components[0] {
            Component::SettingsGroup { items, .. } => items,
            other => panic!("Expected SettingsGroup, got {other:?}"),
        };
        assert_eq!(items.len(), 3, "three consent toggles");
        let on = |id: &str| {
            items
                .iter()
                .find(|i| i.id == id)
                .map(|i| matches!(i.kind, SettingsItemKind::Toggle { enabled } if enabled))
                .unwrap_or(false)
        };
        assert!(on("data_processing"), "data_processing should be on");
        assert!(!on("contact_sharing"), "contact_sharing should be off");
        assert!(on("recovery_vouching"), "recovery_vouching should be on");
    }

    // @internal
    #[test]
    fn consent_cancel_returns_to_overview() {
        let mut e = engine();
        e.step = GdprStep::ManageConsent;
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

    // @internal
    #[test]
    fn overview_shows_cancel_when_deletion_scheduled() {
        let e = GdprEngine::new(Some("Scheduled".into()), "Active".into(), Locale::English)
            .with_deletion_scheduled(true);
        let ids: Vec<String> = e
            .current_screen()
            .actions
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert!(
            ids.contains(&"cancel_deletion".to_string()),
            "scheduled deletion shows the cancel action"
        );
        assert!(
            !ids.contains(&"delete".to_string()),
            "scheduled deletion hides the delete action"
        );
    }

    // @internal
    #[test]
    fn overview_always_offers_panic_shred() {
        let ids: Vec<String> = engine()
            .current_screen()
            .actions
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert!(
            ids.contains(&"panic_shred".to_string()),
            "panic shred is always available"
        );
    }

    // @internal
    #[test]
    fn overview_shows_execute_when_grace_elapsed() {
        let ids: Vec<String> =
            GdprEngine::new(Some("Scheduled".into()), "Active".into(), Locale::English)
                .with_deletion_scheduled(true)
                .with_deletion_executable(true)
                .current_screen()
                .actions
                .iter()
                .map(|a| a.id.clone())
                .collect();
        assert!(
            ids.contains(&"execute_deletion".to_string()),
            "grace elapsed shows the Delete Now action"
        );
    }

    // @internal
    #[test]
    fn confirm_execute_completes_with_execute_action() {
        let mut e = engine();
        e.step = GdprStep::ConfirmExecute;
        let r = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm_execute".into(),
        });
        assert!(matches!(r, ActionResult::Complete));
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(
                crate::ui::GdprChoice::Execute
            ))
        );
    }

    // @internal
    #[test]
    fn confirm_shred_completes_with_shred_action() {
        let mut e = engine();
        e.step = GdprStep::ConfirmShred;
        let r = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm_shred".into(),
        });
        assert!(matches!(r, ActionResult::Complete));
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(crate::ui::GdprChoice::Shred))
        );
    }

    // @internal
    #[test]
    fn consent_status_from_records_uses_latest_decision() {
        use vauchi_core::api::{ConsentRecord, ConsentType};
        let rec = |t: ConsentType, granted: bool, ts: u64| ConsentRecord {
            id: format!("r{ts}"),
            consent_type: t,
            granted,
            timestamp: ts,
            policy_version: None,
        };
        let records = vec![
            rec(ConsentType::DataProcessing, true, 1),
            rec(ConsentType::DataProcessing, false, 2),
            rec(ConsentType::ContactSharing, true, 3),
        ];
        let status = ConsentStatus::from_consent_records(&records);
        assert!(
            !status.data_processing,
            "latest data_processing is a revoke"
        );
        assert!(status.contact_sharing, "contact_sharing granted");
        assert!(!status.recovery_vouching, "never decided defaults to false");
    }

    // @internal
    #[test]
    fn consent_toggle_flips_display_state() {
        let mut e = engine();
        e.step = GdprStep::ManageConsent;
        let result = e.handle_action(UserAction::SettingsToggled {
            component_id: "consent".into(),
            item_id: "data_processing".into(),
        });
        let screen = match result {
            ActionResult::UpdateScreen(s) => s,
            other => panic!("Expected UpdateScreen, got {other:?}"),
        };
        let items = match &screen.components[0] {
            Component::SettingsGroup { items, .. } => items,
            other => panic!("Expected SettingsGroup, got {other:?}"),
        };
        let dp = items.iter().find(|i| i.id == "data_processing").unwrap();
        assert!(
            matches!(dp.kind, SettingsItemKind::Toggle { enabled } if enabled),
            "toggling data_processing should flip it on"
        );
    }
}
