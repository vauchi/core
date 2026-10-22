// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! GDPR / privacy engine — data export, deletion, and consent management.
//! Copy resolves through `i18n::get_string` in the locale threaded at
//! construction (M3 S3b of `2026-07-03-core-screens-bypass-i18n`); keys
//! live in the `privacy.*` + shared `action.*` families.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::device_link_pin::{EnteredPin, PIN_INPUT_ID};
use crate::ui::*;

mod heirloom;

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
    /// Whether an app password is set: an export then asks for it; without
    /// one the user is sent to set one first (#712).
    app_password_set: bool,
    /// The shell can seal a secret behind the OS user-presence check, the
    /// export's verification when no app password is set (#712).
    user_presence_available: bool,
    /// Someone is trusted for recovery, so deleting asks whether to keep
    /// the relay's list of them (#711).
    has_trusted_contacts: bool,
    /// The export waiting on the PIN step: `"export"` or `"heirloom"`.
    pending_export: Option<&'static str>,
    entered_pin: EnteredPin,
    pin_error: Option<String>,
    /// The encrypted export's own password, typed twice (#712).
    export_password: EnteredPin,
    export_password_confirm: EnteredPin,
    export_password_error: Option<String>,
    /// The readable format waiting behind its warning: vCard or JSON.
    readable_export: Option<GdprChoice>,
    locale: Locale,
}

#[derive(Clone, Debug, PartialEq)]
enum GdprStep {
    /// Main privacy settings screen.
    Overview,
    /// Plaintext warning before the paper heirloom export (private#363).
    ConfirmHeirloom,
    /// Deletion confirmation screen showing what will be deleted.
    ConfirmDelete,
    /// Consent-management screen — per-type grant toggles.
    ManageConsent,
    /// Confirm executing a scheduled deletion now (grace elapsed).
    ConfirmExecute,
    /// The session PIN, asked immediately before each export (#712).
    ConfirmExportPin,
    /// No app password is set: set one before exporting (#712).
    ExportNeedsAppPassword,
    /// No app password, but the OS can check the user: waiting on it.
    ExportAwaitPresence,
    /// Verified: encrypted, vCard or JSON (#712).
    ChooseExportFormat,
    /// The encrypted export's own password, typed twice.
    ExportPassword,
    /// The readable formats leave the app unprotected: say so first.
    ReadableExportWarning,
    /// Keep recovery possible or erase everything (#711).
    ChooseRecoveryKeep,
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
            app_password_set: false,
            user_presence_available: false,
            has_trusted_contacts: false,
            pending_export: None,
            entered_pin: EnteredPin::default(),
            pin_error: None,
            export_password: EnteredPin::default(),
            export_password_confirm: EnteredPin::default(),
            export_password_error: None,
            readable_export: None,
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

    /// Whether the shell can run the OS user-presence check (#712).
    pub fn with_user_presence_available(mut self, available: bool) -> Self {
        self.user_presence_available = available;
        self
    }

    /// Whether an app password is set, which the export asks for (#712).
    pub fn with_app_password_set(mut self, set: bool) -> Self {
        self.app_password_set = set;
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
                    icon: Some("shield".into()),
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
                            icon: Some("folder".into()),
                            detail: Some(self.t("privacy.view_data_desc")),
                            a11y: Some(A11y::labeled(self.t("privacy.view_data"))),
                            info_key: None,
                        },
                        ActionListItem {
                            id: "manage_consent".into(),
                            label: self.t("privacy.manage_consent"),
                            icon: Some("checkmark.seal".into()),
                            detail: Some(self.t("privacy.manage_consent_desc")),
                            a11y: Some(A11y::labeled(self.t("privacy.manage_consent"))),
                            info_key: None,
                        },
                    ],
                },
            ],
            contextual_actions: {
                let mut actions = vec![
                    ScreenAction {
                        id: "export".into(),
                        label: self.t("privacy.export_data"),
                        style: ActionStyle::Primary,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("privacy.export_data"))),
                    },
                    ScreenAction {
                        id: "heirloom".into(),
                        label: self.t("privacy.heirloom.action"),
                        style: ActionStyle::Secondary,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("privacy.heirloom.action"))),
                    },
                ];
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
                        style: ActionStyle::Serious,
                        enabled: true,
                        a11y: Some(A11y::labeled(self.t("privacy.delete_identity"))),
                    });
                }
                actions
            },
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
                        subtitle: None,
                        label: self.t("privacy.consent_data_processing"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.data_processing,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_data_processing"))),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "contact_sharing".into(),
                        subtitle: None,
                        label: self.t("privacy.consent_contact_sharing"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.contact_sharing,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_contact_sharing"))),
                        info_key: None,
                    },
                    SettingsItem {
                        id: "recovery_vouching".into(),
                        subtitle: None,
                        label: self.t("privacy.consent_recovery_vouching"),
                        kind: SettingsItemKind::Toggle {
                            enabled: self.consent.recovery_vouching,
                        },
                        a11y: Some(A11y::labeled(self.t("privacy.consent_recovery_vouching"))),
                        info_key: None,
                    },
                ],
            }],
            contextual_actions: vec![ScreenAction {
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
            contextual_actions: vec![
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

    fn build_screen(&self) -> ScreenModel {
        match self.step {
            GdprStep::Overview => self.build_overview(),
            GdprStep::ConfirmDelete => self.build_confirm_delete(),
            GdprStep::ManageConsent => self.build_consent(),
            GdprStep::ConfirmExecute => self.build_confirm_execute(),
            GdprStep::ConfirmHeirloom => self.build_confirm_heirloom(),
            GdprStep::ConfirmExportPin => self.build_confirm_export_pin(),
            GdprStep::ExportNeedsAppPassword => self.build_export_needs_app_password(),
            GdprStep::ExportAwaitPresence => self.build_export_await_presence(),
            GdprStep::ChooseExportFormat => self.build_choose_export_format(),
            GdprStep::ExportPassword => self.build_export_password(),
            GdprStep::ReadableExportWarning => self.build_readable_export_warning(),
            GdprStep::ChooseRecoveryKeep => self.build_recovery_choice(),
        }
    }
}

impl WorkflowEngine for GdprEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn can_navigate_back_within(&self) -> bool {
        self.in_export()
    }

    /// Back from any export step ends the export on the overview (#750).
    fn navigate_back_within(&mut self) -> bool {
        if !self.in_export() {
            return false;
        }
        self.abandon_export();
        self.step = GdprStep::Overview;
        true
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        if let Some(result) = self.handle_export_step(&action) {
            return result;
        }
        match (&self.step, action) {
            // Overview: export first asks for the session PIN (#712)
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "export" =>
            {
                self.begin_export("export")
            }
            // Overview: the heirloom first shows its plaintext warning
            (GdprStep::Overview, UserAction::ActionPressed { action_id })
                if action_id == "heirloom" =>
            {
                self.step = GdprStep::ConfirmHeirloom;
                ActionResult::NavigateTo(self.build_screen())
            }
            // Heirloom warning: confirming asks for the session PIN (#712)
            (GdprStep::ConfirmHeirloom, UserAction::ActionPressed { action_id })
                if action_id == "confirm_heirloom" =>
            {
                self.begin_export("heirloom")
            }
            (
                GdprStep::ConfirmExportPin,
                UserAction::TextChanged {
                    component_id,
                    value,
                },
            ) if component_id == PIN_INPUT_ID => {
                self.entered_pin.set(value);
                ActionResult::UpdateScreen(self.build_screen())
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
            // Confirmation: confirm triggers deletion
            (GdprStep::ConfirmDelete, UserAction::ActionPressed { action_id })
                if action_id == "confirm_delete" =>
            {
                self.confirm_delete()
            }
            (GdprStep::ChooseRecoveryKeep, UserAction::ActionPressed { action_id })
                if action_id != "cancel" =>
            {
                self.choose_recovery(&action_id)
            }
            // Execute confirmation: confirm triggers immediate execution
            (GdprStep::ConfirmExecute, UserAction::ActionPressed { action_id })
                if action_id == "confirm_execute" =>
            {
                self.last_action = Some("execute".into());
                ActionResult::Complete
            }
            // Any confirmation screen: cancel goes back to overview
            (
                GdprStep::ConfirmDelete
                | GdprStep::ConfirmExecute
                | GdprStep::ConfirmHeirloom
                | GdprStep::ConfirmExportPin
                | GdprStep::ExportNeedsAppPassword
                | GdprStep::ExportAwaitPresence
                | GdprStep::ChooseExportFormat
                | GdprStep::ExportPassword
                | GdprStep::ReadableExportWarning
                | GdprStep::ChooseRecoveryKeep,
                UserAction::ActionPressed { action_id },
            ) if action_id == "cancel" => {
                self.abandon_export();
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
        match self.step {
            GdprStep::ConfirmExportPin if !self.entered_pin.as_str().is_empty() => {
                return Some(crate::ui::EngineOutput::ExportPin {
                    pin: self.entered_pin.as_str().to_owned(),
                });
            }
            GdprStep::ExportAwaitPresence => {
                return Some(crate::ui::EngineOutput::AwaitingUserPresence);
            }
            GdprStep::ConfirmExportPin | GdprStep::ExportNeedsAppPassword => return None,
            _ => {}
        }
        if self.last_action.as_deref() == Some("export_encrypted") {
            return Some(crate::ui::EngineOutput::GdprExportEncrypted {
                password: self.export_password.as_str().to_owned(),
            });
        }
        let choice = match self.last_action.as_deref() {
            Some("export") => GdprChoice::Export,
            Some("delete") => GdprChoice::Delete,
            Some("delete_erasing_everything") => GdprChoice::DeleteErasingEverything,
            Some("cancel_deletion") => GdprChoice::CancelDeletion,
            Some("execute") => GdprChoice::Execute,
            Some("heirloom") => GdprChoice::Heirloom,
            Some("export_vcard") => GdprChoice::ExportVcard,
            _ => return None,
        };
        Some(crate::ui::EngineOutput::Gdpr(choice))
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        match update {
            crate::ui::EngineUpdate::GdprExport(update) => self.apply_export_update(update),
            _ => false,
        }
    }
}

#[path = "gdpr_deletion.rs"]
mod deletion;
#[path = "gdpr_export.rs"]
mod export;
#[cfg(feature = "network-rustls")]
pub(crate) use export::SET_APP_PASSWORD_ACTION_ID;

// INLINE_TEST_REQUIRED: Tests access private GdprStep enum and internal engine state
#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> GdprEngine {
        GdprEngine::new(None, "Active".into(), Locale::ENGLISH).with_deletion_summary(
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

    // @internal
    #[test]
    fn scheduling_deletion_is_serious_and_only_executing_it_is_destructive() {
        let e = engine();
        let screen = e.current_screen();
        let delete = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "delete")
            .expect("delete action present");
        assert_eq!(delete.style, ActionStyle::Serious);
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
        let mut e = GdprEngine::new(None, "Active".into(), Locale::ENGLISH).with_deletion_summary(
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
    fn confirm_delete_without_trusted_contacts_erases_everything() {
        let mut e = engine();
        e.step = GdprStep::ConfirmDelete;
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "confirm_delete".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(
            e.engine_output(),
            Some(crate::ui::EngineOutput::Gdpr(
                crate::ui::GdprChoice::DeleteErasingEverything
            ))
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
    fn export_waits_on_the_pin_and_names_no_choice_until_verified() {
        let mut e = engine().with_app_password_set(true);
        let result = e.handle_action(UserAction::ActionPressed {
            action_id: "export".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(e.step, GdprStep::ConfirmExportPin);
        assert_eq!(e.engine_output(), None);
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
            GdprEngine::new(None, "Active".into(), Locale::ENGLISH).with_consent(ConsentStatus {
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
        let e = GdprEngine::new(Some("Scheduled".into()), "Active".into(), Locale::ENGLISH)
            .with_deletion_scheduled(true);
        let ids: Vec<String> = e
            .current_screen()
            .contextual_actions
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

    // The wipe has one home, Settings → Advanced, behind the typed
    // confirmation of the shred screen (vauchi/private#279, #598).
    // @internal
    #[test]
    fn overview_offers_no_wipe() {
        let ids: Vec<String> = engine()
            .current_screen()
            .contextual_actions
            .iter()
            .map(|a| a.id.clone())
            .collect();
        assert_eq!(ids, ["export", "heirloom", "delete"]);
    }

    // @internal
    #[test]
    fn overview_shows_execute_when_grace_elapsed() {
        let ids: Vec<String> =
            GdprEngine::new(Some("Scheduled".into()), "Active".into(), Locale::ENGLISH)
                .with_deletion_scheduled(true)
                .with_deletion_executable(true)
                .current_screen()
                .contextual_actions
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
