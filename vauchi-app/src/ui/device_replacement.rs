// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device replacement wizard — guides the user through replacing a device.
//!
//! Three flows share one state machine:
//! - **Flow A** (old device available): guided device link + progress + decommission
//! - **Flow B** (old device lost): post-restore guidance + social recovery
//! - **Flow C** (proactive): Settings entry → select mode → delegate to existing flows

use crate::ui::*;
use vauchi_core::exchange::{ExchangeCommand, FilePickPurpose};

/// MIME types for the encrypted backup file picker. Mirrors
/// `onboarding::backup_mime_types` — the picker drives an identical
/// import flow regardless of entry point.
fn backup_mime_types() -> Vec<String> {
    vec!["application/octet-stream".into(), "text/plain".into()]
}

/// Which side of the replacement flow this device is on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplacementRole {
    /// This is the OLD device setting up the new one (Settings → "Set Up New Device").
    Source,
    /// This is the NEW device receiving data (onboarding → "Transfer from another device").
    Target,
    /// Post-restore guidance (old device lost, backup restored).
    PostRestore,
}

/// What the user chose on the decommission screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// User chose to unlink this (old) device.
    RemoveOldDevice,
    /// User chose to keep both devices linked.
    KeepBoth,
    /// User cancelled or navigated back.
    Cancelled,
}

/// Steps in the device replacement flow.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// Choose role: "I have my old device" or "I lost my old device"
    SelectMode,
    /// Old device: showing QR for new device to scan
    ShowQr,
    /// Old device: verify code with new device
    VerifyCode,
    /// Both: syncing contacts and ratchet states
    Syncing,
    /// Both: transfer complete, show summary
    Complete,
    /// Old device: offer to decommission
    Decommission,
    /// Old device: confirm decommission (InlineConfirm per ADR-022)
    ConfirmDecommission,
    /// Post-restore: explain contact loss and recovery options
    RestoreGuidance,
}

/// Engine that drives the device replacement wizard.
#[derive(Clone, Debug)]
pub struct DeviceReplacementEngine {
    step: Step,
    role: ReplacementRole,
    qr_data: Option<String>,
    verification_code: Option<String>,
    synced_contacts: u32,
    total_contacts: u32,
    outcome: CompletionOutcome,
    cancelled: bool,
}

impl DeviceReplacementEngine {
    /// Creates a new engine for the source (old) device side.
    pub fn new_source() -> Self {
        Self::new(ReplacementRole::Source, Step::ShowQr)
    }

    /// Creates a new engine for the target (new) device — shows mode selection.
    pub fn new_target() -> Self {
        Self::new(ReplacementRole::Target, Step::SelectMode)
    }

    /// Creates a new engine for post-restore guidance (old device lost).
    pub fn new_post_restore() -> Self {
        Self::new(ReplacementRole::PostRestore, Step::RestoreGuidance)
    }

    fn new(role: ReplacementRole, step: Step) -> Self {
        Self {
            step,
            role,
            qr_data: None,
            verification_code: None,
            synced_contacts: 0,
            total_contacts: 0,
            outcome: CompletionOutcome::Cancelled,
            cancelled: false,
        }
    }

    /// Set the QR data for the source device to display.
    pub fn set_qr_data(&mut self, qr_data: String) {
        self.qr_data = Some(qr_data);
    }

    /// Signal that a peer device has connected, providing the verification code.
    pub fn peer_connected(&mut self, verification_code: String) {
        if self.step == Step::ShowQr {
            self.verification_code = Some(verification_code);
            self.step = Step::VerifyCode;
        }
    }

    /// Update sync progress.
    pub fn sync_progress(&mut self, synced: u32, total: u32) {
        if self.step == Step::Syncing {
            self.synced_contacts = synced;
            self.total_contacts = total;
        }
    }

    /// Signal that data sync has completed.
    pub fn sync_complete(&mut self, synced: u32, total: u32) {
        if self.step == Step::Syncing {
            self.synced_contacts = synced;
            self.total_contacts = total;
            self.step = Step::Complete;
        }
    }

    /// Returns the completion outcome chosen by the user.
    pub fn completion_outcome(&self) -> &CompletionOutcome {
        &self.outcome
    }

    fn step_number(&self) -> u8 {
        match self.step {
            Step::SelectMode => 1,
            Step::ShowQr => 1,
            Step::VerifyCode => 2,
            Step::Syncing => 3,
            Step::Complete => 4,
            Step::Decommission | Step::ConfirmDecommission => 5,
            Step::RestoreGuidance => 1,
        }
    }

    fn total_steps(&self) -> u8 {
        match self.role {
            ReplacementRole::Source => 5,
            ReplacementRole::Target => 4,
            ReplacementRole::PostRestore => 1,
        }
    }

    fn progress(&self) -> Option<Progress> {
        if self.role == ReplacementRole::PostRestore {
            return None;
        }
        Some(Progress {
            current_step: self.step_number(),
            total_steps: self.total_steps(),
            label: None,
        })
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            Step::SelectMode => self.build_select_mode(),
            Step::ShowQr => self.build_show_qr(),
            Step::VerifyCode => self.build_verify_code(),
            Step::Syncing => self.build_syncing(),
            Step::Complete => self.build_complete(),
            Step::Decommission => self.build_decommission(),
            Step::ConfirmDecommission => self.build_confirm_decommission(),
            Step::RestoreGuidance => self.build_restore_guidance(),
        }
    }

    fn build_select_mode(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "replacement_select_mode".into(),
            title: "Transfer from another device".into(),
            subtitle: Some("Do you have your old device nearby?".into()),
            components: vec![Component::InfoPanel {
                id: "mode_info".into(),
                icon: Some("devices".into()),
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("checkmark".into()),
                        title: "Yes, I have my old device".into(),
                        detail: "Transfer all contacts and data via QR code.".into(),
                    },
                    InfoItem {
                        icon: Some("xmark".into()),
                        title: "No, I lost my old device".into(),
                        detail:
                            "Restore identity from backup. Contacts will need to be re-established."
                                .into(),
                    },
                ],
                a11y: Some(A11y {
                    label: Some("Choose transfer method".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: "has_old_device".into(),
                    label: "Transfer via QR".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "lost_device".into(),
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
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_show_qr(&self) -> ScreenModel {
        let qr_data = self.qr_data.clone().unwrap_or_default();
        ScreenModel {
            screen_id: "replacement_show_qr".into(),
            title: "Set Up New Device".into(),
            subtitle: Some("Scan this QR code on your new device.".into()),
            components: vec![
                Component::QrCode {
                    id: "qr".into(),
                    data: qr_data,
                    mode: QrMode::Display,
                    label: Some("Scan on new device".into()),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some("Device transfer QR code".into()),
                        hint: Some("Scan this code on your new device to begin transfer.".into()),
                        role: Some(AccessibilityRole::Image),
                    }),
                },
                Component::InfoPanel {
                    id: "qr_instructions".into(),
                    icon: Some("info".into()),
                    title: "".into(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Keep both devices nearby".into(),
                        detail: "The transfer requires both devices to be in proximity.".into(),
                    }],
                    a11y: None,
                },
            ],
            actions: vec![ScreenAction {
                id: "cancel".into(),
                label: "Cancel".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_verify_code(&self) -> ScreenModel {
        let code = self.verification_code.as_deref().unwrap_or("------");
        ScreenModel {
            screen_id: "replacement_verify".into(),
            title: "Verify Device".into(),
            subtitle: None,
            components: vec![
                Component::Text {
                    id: "code".into(),
                    content: code.to_string(),
                    style: TextStyle::Title,
                },
                Component::InfoPanel {
                    id: "verify_info".into(),
                    icon: Some("shield".into()),
                    title: "Compare verification codes".into(),
                    items: vec![InfoItem {
                        icon: None,
                        title: "Both devices must show the same code".into(),
                        detail: "This confirms you are connecting the right devices.".into(),
                    }],
                    a11y: None,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "confirm".into(),
                    label: "Codes match".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "reject".into(),
                    label: "Codes don't match".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_syncing(&self) -> ScreenModel {
        let detail = if self.total_contacts > 0 {
            Some(format!(
                "Syncing {}/{} contacts...",
                self.synced_contacts, self.total_contacts
            ))
        } else {
            Some("Syncing data...".into())
        };
        ScreenModel {
            screen_id: "replacement_syncing".into(),
            title: "Transferring".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "syncing".into(),
                icon: None,
                title: "Transfer in progress".into(),
                detail,
                status: Status::InProgress,
                a11y: Some(A11y {
                    label: Some("Transfer progress".into()),
                    hint: Some("Data is being transferred to the new device.".into()),
                    role: None,
                }),
            }],
            actions: vec![],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_complete(&self) -> ScreenModel {
        let detail = if self.total_contacts > 0 {
            Some(format!(
                "{} contacts transferred successfully.",
                self.synced_contacts
            ))
        } else {
            Some("All data transferred.".into())
        };
        let actions = if self.role == ReplacementRole::Source {
            vec![ScreenAction {
                id: "decommission".into(),
                label: "Continue".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }]
        } else {
            vec![ScreenAction {
                id: "done".into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }]
        };
        ScreenModel {
            screen_id: "replacement_complete".into(),
            title: "Transfer Complete".into(),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "complete".into(),
                icon: None,
                title: "Transfer complete".into(),
                detail,
                status: Status::Success,
                a11y: Some(A11y {
                    label: Some("Transfer complete".into()),
                    hint: Some("All data has been transferred to the new device.".into()),
                    role: None,
                }),
            }],
            actions,
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_decommission(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "replacement_decommission".into(),
            title: "Old Device".into(),
            subtitle: Some("What would you like to do with this device?".into()),
            components: vec![Component::InfoPanel {
                id: "decommission_info".into(),
                icon: Some("warning".into()),
                title: "".into(),
                items: vec![
                    InfoItem {
                        icon: Some("trash".into()),
                        title: "Remove this device".into(),
                        detail: "Unlink this device from your identity. Choose this if you're giving away or recycling this phone.".into(),
                    },
                    InfoItem {
                        icon: Some("devices".into()),
                        title: "Keep both devices".into(),
                        detail: "Both devices stay linked and receive updates.".into(),
                    },
                ],
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "remove_old".into(),
                    label: "Remove this device".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "keep_both".into(),
                    label: "Keep both".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }

    /// ADR-022: InlineConfirm for the irrevocable decommission action.
    fn build_confirm_decommission(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "replacement_confirm_decommission".into(),
            title: "Confirm Removal".into(),
            subtitle: None,
            components: vec![Component::InlineConfirm {
                id: "remove".into(),
                warning: "This device will be unlinked from your identity. This cannot be undone."
                    .into(),
                confirm_text: "Remove this device".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: Some(A11y {
                    label: Some("Confirm device removal".into()),
                    hint: Some("Permanently unlinks this device from your identity.".into()),
                    role: None,
                }),
            }],
            actions: vec![],
            progress: self.progress(),
            ..Default::default()
        }
    }

    fn build_restore_guidance(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "replacement_restore_guidance".into(),
            title: "Contacts Need Re-Establishing".into(),
            subtitle: Some(
                "Your identity is restored, but contacts cannot be recovered from a backup.".into(),
            ),
            components: vec![Component::InfoPanel {
                id: "restore_options".into(),
                icon: Some("info".into()),
                title: "How to recover your contacts".into(),
                items: vec![
                    InfoItem {
                        icon: Some("people".into()),
                        title: "Ask a trusted contact to vouch for you".into(),
                        detail: "Use social recovery to restore contacts through a trusted friend."
                            .into(),
                    },
                    InfoItem {
                        icon: Some("qrcode".into()),
                        title: "Meet contacts in person".into(),
                        detail: "Exchange contact cards again via QR code.".into(),
                    },
                ],
                a11y: Some(A11y {
                    label: Some("Contact recovery options".into()),
                    hint: None,
                    role: Some(AccessibilityRole::Heading),
                }),
            }],
            actions: vec![
                ScreenAction {
                    id: "social_recovery".into(),
                    label: "Social Recovery".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                },
                ScreenAction {
                    id: "done".into(),
                    label: "I'll do this later".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: self.progress(),
            ..Default::default()
        }
    }
}

impl WorkflowEngine for DeviceReplacementEngine {
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn as_any_mut(&mut self) -> Option<&mut dyn std::any::Any> {
        Some(self)
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match action {
            UserAction::ActionPressed { action_id } => match self.step {
                Step::SelectMode => match action_id.as_str() {
                    "has_old_device" => ActionResult::StartDeviceLink,
                    "lost_device" => ActionResult::ExchangeCommands {
                        commands: vec![ExchangeCommand::FilePickFromUser {
                            accepted_mime_types: backup_mime_types(),
                            purpose: FilePickPurpose::ImportBackup,
                        }],
                    },
                    "back" => {
                        self.cancelled = true;
                        ActionResult::Complete
                    }
                    _ => ActionResult::UpdateScreen(self.build_screen()),
                },
                Step::ShowQr if action_id == "cancel" => {
                    self.cancelled = true;
                    ActionResult::Complete
                }
                Step::VerifyCode if action_id == "confirm" => {
                    self.step = Step::Syncing;
                    ActionResult::NavigateTo(self.build_screen())
                }
                Step::VerifyCode if action_id == "reject" => {
                    self.step = Step::ShowQr;
                    self.verification_code = None;
                    ActionResult::NavigateTo(self.build_screen())
                }
                Step::Complete if action_id == "decommission" => {
                    self.step = Step::Decommission;
                    ActionResult::NavigateTo(self.build_screen())
                }
                Step::Complete if action_id == "done" => {
                    self.outcome = CompletionOutcome::KeepBoth;
                    ActionResult::Complete
                }
                Step::Decommission if action_id == "remove_old" => {
                    // ADR-022: show InlineConfirm before irrevocable action
                    self.step = Step::ConfirmDecommission;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                Step::Decommission if action_id == "keep_both" => {
                    self.outcome = CompletionOutcome::KeepBoth;
                    ActionResult::Complete
                }
                Step::ConfirmDecommission if action_id == "confirm_remove" => {
                    self.outcome = CompletionOutcome::RemoveOldDevice;
                    ActionResult::Complete
                }
                Step::ConfirmDecommission if action_id == "cancel_remove" => {
                    self.step = Step::Decommission;
                    ActionResult::UpdateScreen(self.build_screen())
                }
                Step::RestoreGuidance if action_id == "social_recovery" => {
                    self.outcome = CompletionOutcome::KeepBoth;
                    ActionResult::Complete
                }
                Step::RestoreGuidance if action_id == "done" => {
                    self.outcome = CompletionOutcome::KeepBoth;
                    ActionResult::Complete
                }
                _ => ActionResult::UpdateScreen(self.build_screen()),
            },
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: Tests access private Step enum and DeviceReplacementEngine internals
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn source_starts_at_show_qr() {
        let engine = DeviceReplacementEngine::new_source();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "replacement_show_qr");
    }

    // @internal
    #[test]
    fn target_starts_at_select_mode() {
        let engine = DeviceReplacementEngine::new_target();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "replacement_select_mode");
    }

    // @internal
    #[test]
    fn post_restore_starts_at_guidance() {
        let engine = DeviceReplacementEngine::new_post_restore();
        let screen = engine.current_screen();
        assert_eq!(screen.screen_id, "replacement_restore_guidance");
    }

    // @internal
    #[test]
    fn peer_connected_only_from_show_qr() {
        let mut engine = DeviceReplacementEngine::new_source();
        assert_eq!(engine.current_screen().screen_id, "replacement_show_qr");

        engine.peer_connected("123-456".into());
        assert_eq!(engine.current_screen().screen_id, "replacement_verify");

        // Calling again from VerifyCode should be ignored
        engine.peer_connected("789-012".into());
        assert_eq!(engine.current_screen().screen_id, "replacement_verify");
    }

    // @internal
    #[test]
    fn sync_complete_only_from_syncing() {
        let mut engine = DeviceReplacementEngine::new_source();
        // Not in Syncing state — should be ignored
        engine.sync_complete(10, 10);
        assert_eq!(engine.current_screen().screen_id, "replacement_show_qr");

        // Move to Syncing
        engine.peer_connected("123-456".into());
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        assert!(matches!(result, ActionResult::NavigateTo(_)));
        assert_eq!(engine.current_screen().screen_id, "replacement_syncing");

        // Now sync_complete should work
        engine.sync_complete(42, 50);
        assert_eq!(engine.current_screen().screen_id, "replacement_complete");
        assert_eq!(engine.synced_contacts, 42);
        assert_eq!(engine.total_contacts, 50);
    }

    // @internal
    #[test]
    fn sync_progress_only_from_syncing() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.sync_progress(5, 10);
        assert_eq!(engine.synced_contacts, 0); // ignored, not in Syncing

        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_progress(5, 10);
        assert_eq!(engine.synced_contacts, 5);
        assert_eq!(engine.total_contacts, 10);
    }

    // @internal
    #[test]
    fn decommission_requires_inline_confirm() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_complete(10, 10);

        // Move to Decommission
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "decommission".into(),
        });
        assert_eq!(
            engine.current_screen().screen_id,
            "replacement_decommission"
        );

        // Press "remove_old" — should show InlineConfirm, NOT complete
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "remove_old".into(),
        });
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
        assert_eq!(
            engine.current_screen().screen_id,
            "replacement_confirm_decommission"
        );

        // Has InlineConfirm component
        let screen = engine.current_screen();
        assert!(screen.components.iter().any(|c| matches!(c,
            Component::InlineConfirm { id, destructive, .. }
            if id == "remove" && *destructive
        )));
    }

    // @internal
    #[test]
    fn confirm_decommission_sets_outcome() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_complete(10, 10);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "decommission".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "remove_old".into(),
        });

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm_remove".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(
            engine.completion_outcome(),
            &CompletionOutcome::RemoveOldDevice
        );
    }

    // @internal
    #[test]
    fn cancel_decommission_returns_to_options() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_complete(10, 10);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "decommission".into(),
        });
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "remove_old".into(),
        });
        assert_eq!(
            engine.current_screen().screen_id,
            "replacement_confirm_decommission"
        );

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel_remove".into(),
        });
        assert_eq!(
            engine.current_screen().screen_id,
            "replacement_decommission"
        );
    }

    // @internal
    #[test]
    fn keep_both_sets_outcome() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_complete(10, 10);
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "decommission".into(),
        });

        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "keep_both".into(),
        });
        assert!(matches!(result, ActionResult::Complete));
        assert_eq!(engine.completion_outcome(), &CompletionOutcome::KeepBoth);
    }

    // @internal
    #[test]
    fn cancel_sets_was_cancelled() {
        let mut engine = DeviceReplacementEngine::new_source();
        assert!(!engine.was_cancelled());

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "cancel".into(),
        });
        assert!(engine.was_cancelled());
    }

    // @internal
    #[test]
    fn select_mode_back_sets_cancelled() {
        let mut engine = DeviceReplacementEngine::new_target();
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "back".into(),
        });
        assert!(engine.was_cancelled());
    }

    // @internal
    #[test]
    fn select_mode_has_old_device_starts_link() {
        let mut engine = DeviceReplacementEngine::new_target();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "has_old_device".into(),
        });
        assert!(matches!(result, ActionResult::StartDeviceLink));
    }

    // @internal
    #[test]
    fn select_mode_lost_device_emits_file_pick_for_backup() {
        let mut engine = DeviceReplacementEngine::new_target();
        let result = engine.handle_action(UserAction::ActionPressed {
            action_id: "lost_device".into(),
        });
        match result {
            ActionResult::ExchangeCommands { commands } => {
                assert_eq!(commands.len(), 1);
                match &commands[0] {
                    ExchangeCommand::FilePickFromUser { purpose, .. } => {
                        assert_eq!(*purpose, FilePickPurpose::ImportBackup);
                    }
                    other => panic!("expected FilePickFromUser, got {:?}", other),
                }
            }
            other => panic!(
                "expected ExchangeCommands(FilePickFromUser/ImportBackup), got {:?}",
                other
            ),
        }
    }

    // @internal
    #[test]
    fn reject_code_returns_to_qr() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        assert_eq!(engine.current_screen().screen_id, "replacement_verify");

        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "reject".into(),
        });
        assert_eq!(engine.current_screen().screen_id, "replacement_show_qr");
        assert!(engine.verification_code.is_none());
    }

    // @internal
    #[test]
    fn syncing_shows_progress_detail() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_progress(7, 20);

        let screen = engine.current_screen();
        let detail = match &screen.components[0] {
            Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => panic!("Expected StatusIndicator"),
        };
        assert_eq!(detail, Some("Syncing 7/20 contacts...".into()));
    }

    // @internal
    #[test]
    fn complete_shows_contact_count() {
        let mut engine = DeviceReplacementEngine::new_source();
        engine.peer_connected("123-456".into());
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "confirm".into(),
        });
        engine.sync_complete(42, 50);

        let screen = engine.current_screen();
        let detail = match &screen.components[0] {
            Component::StatusIndicator { detail, .. } => detail.clone(),
            _ => panic!("Expected StatusIndicator"),
        };
        assert_eq!(detail, Some("42 contacts transferred successfully.".into()));
    }

    // @internal
    #[test]
    fn progress_steps_correct_for_source() {
        let engine = DeviceReplacementEngine::new_source();
        let progress = engine.progress().unwrap();
        assert_eq!(progress.current_step, 1);
        assert_eq!(progress.total_steps, 5);
    }

    // @internal
    #[test]
    fn progress_steps_correct_for_target() {
        let engine = DeviceReplacementEngine::new_target();
        let progress = engine.progress().unwrap();
        assert_eq!(progress.current_step, 1);
        assert_eq!(progress.total_steps, 4);
    }

    // @internal
    #[test]
    fn post_restore_has_no_progress() {
        let engine = DeviceReplacementEngine::new_post_restore();
        assert!(engine.progress().is_none());
    }
}
