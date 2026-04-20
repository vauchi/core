// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social recovery engine — outgoing recovery flow.
//!
//! State machine: Status → ShowClaimQr → CollectVouchers → Complete.
//! The recovering user shows a claim QR, collects voucher scans from
//! guardians, and submits the proof when the threshold is met.

use crate::ui::*;

/// Steps in the outgoing recovery workflow.
#[derive(Clone, Debug, PartialEq)]
enum RecoveryStep {
    /// Initial screen: quorum status + trusted contacts list.
    Status,
    /// Display the recovery claim as a QR code for guardians to scan.
    ShowClaimQr,
    /// Collecting vouchers from guardians. Shows progress toward threshold.
    CollectVouchers,
    /// Recovery proof submitted successfully.
    Complete,
}

/// A voucher collected during recovery (display-only).
#[derive(Clone, Debug)]
struct CollectedVoucher {
    /// Display name of the guardian who vouched.
    guardian_name: String,
}

/// Engine that drives the outgoing social recovery flow.
///
/// ADR-021 compliant: all UI is described via `ScreenModel`.
#[derive(Clone, Debug)]
pub struct RecoveryEngine {
    step: RecoveryStep,
    trusted_contacts: Vec<ContactItem>,
    quorum_threshold: usize,
    /// Number of linked devices (0 = this is the only device).
    linked_device_count: usize,
    /// Claim data (old_pk) set before starting recovery.
    claim_data: Option<[u8; 32]>,
    /// Vouchers collected so far (display records).
    collected_vouchers: Vec<CollectedVoucher>,
}

impl RecoveryEngine {
    pub fn new(trusted_contacts: Vec<ContactItem>, quorum_threshold: usize) -> Self {
        Self {
            step: RecoveryStep::Status,
            trusted_contacts,
            quorum_threshold,
            linked_device_count: 0,
            claim_data: None,
            collected_vouchers: Vec::new(),
        }
    }

    /// Sets the number of linked devices (other than the current one).
    pub fn set_linked_device_count(&mut self, count: usize) {
        self.linked_device_count = count;
    }

    /// Sets the old_pk claim data. Called by AppEngine before the user
    /// starts recovery (from `create_recovery_claim()`).
    pub fn set_claim_data(&mut self, old_pk: [u8; 32]) {
        self.claim_data = Some(old_pk);
    }

    /// Adds a voucher for testing purposes. Production code uses
    /// `handle_hardware_event` with scanned voucher data.
    #[doc(hidden)]
    pub fn add_voucher_for_testing(&mut self, guardian_name: &str) {
        self.collected_vouchers.push(CollectedVoucher {
            guardian_name: guardian_name.into(),
        });
    }

    fn threshold_met(&self) -> bool {
        self.collected_vouchers.len() >= self.quorum_threshold
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            RecoveryStep::Status => self.build_status_screen(),
            RecoveryStep::ShowClaimQr => self.build_claim_qr_screen(),
            RecoveryStep::CollectVouchers => self.build_collect_screen(),
            RecoveryStep::Complete => self.build_complete_screen(),
        }
    }

    fn build_status_screen(&self) -> ScreenModel {
        let current = self.trusted_contacts.len();
        let quorum_met = current >= self.quorum_threshold;

        let mut components = Vec::new();

        // Multi-device awareness: if user has other devices, suggest
        // revoking from a surviving device instead of full recovery.
        if self.linked_device_count > 0 {
            components.push(Component::StatusIndicator {
                id: "multi_device_hint".into(),
                icon: Some("link".into()),
                title: "Linked Devices Available".into(),
                detail: Some(format!(
                    "You have {} other linked device(s). If you lost one device \
                     but still have another, revoke the lost device from \
                     Device Management instead of using recovery.",
                    self.linked_device_count
                )),
                status: Status::Success,
                a11y: None,
            });
        }

        components.push(Component::InfoPanel {
            id: "quorum_info".into(),
            icon: Some("recovery".into()),
            title: "Quorum Status".into(),
            items: vec![
                InfoItem {
                    icon: None,
                    title: "Trusted Contacts".into(),
                    detail: format!("{current} of {}", self.quorum_threshold),
                },
                InfoItem {
                    icon: None,
                    title: "Quorum Met".into(),
                    detail: if quorum_met { "Yes" } else { "No" }.into(),
                },
            ],
            a11y: None,
        });

        components.push(Component::ContactList {
            id: "trusted_contacts".into(),
            contacts: self.trusted_contacts.clone(),
            searchable: false,
        });

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: "Social Recovery".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "start_recovery".into(),
                    label: "Start Recovery".into(),
                    style: ActionStyle::Primary,
                    enabled: quorum_met,
                    a11y: None,
                },
                ScreenAction {
                    id: "check_status".into(),
                    label: "Check Status".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    fn build_claim_qr_screen(&self) -> ScreenModel {
        let qr_data = self.claim_data.map(hex::encode).unwrap_or_default();

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: "Recovery Claim".into(),
            subtitle: Some("Show this QR code to your trusted contacts".into()),
            components: vec![
                Component::QrCode {
                    id: "claim_qr".into(),
                    data: qr_data,
                    mode: QrMode::Display,
                    label: Some("Recovery claim — scan to vouch".into()),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some("Recovery claim QR code".into()),
                        hint: Some("Show this to trusted contacts so they can vouch for you".into()),
                        role: None,
                    }),
                },
                Component::Text {
                    id: "claim_instructions".into(),
                    content: "Meet each trusted contact in person. They scan this code to create a voucher for you.".into(),
                    style: TextStyle::Body,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "wait_for_voucher".into(),
                    label: "Collect Vouchers".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
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

    fn build_collect_screen(&self) -> ScreenModel {
        let count = self.collected_vouchers.len();
        let threshold = self.quorum_threshold;
        let met = self.threshold_met();

        let status = if met {
            Status::Success
        } else {
            Status::InProgress
        };

        let mut components: Vec<Component> = vec![Component::StatusIndicator {
            id: "voucher_progress".into(),
            icon: Some("recovery".into()),
            title: "Voucher Collection".into(),
            detail: Some(format!("{count} of {threshold} vouchers collected")),
            status,
            a11y: None,
        }];

        // Show collected voucher names
        if !self.collected_vouchers.is_empty() {
            let items: Vec<ActionListItem> = self
                .collected_vouchers
                .iter()
                .enumerate()
                .map(|(i, v)| ActionListItem {
                    id: format!("voucher_{i}"),
                    label: v.guardian_name.clone(),
                    icon: Some("checkmark.circle".into()),
                    detail: Some("Vouched".into()),
                    a11y: None,
                    info_key: None,
                })
                .collect();
            components.push(Component::ActionList {
                id: "collected_vouchers".into(),
                items,
            });
        }

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: "Collecting Vouchers".into(),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "submit_proof".into(),
                    label: "Submit Proof".into(),
                    style: ActionStyle::Primary,
                    enabled: met,
                    a11y: None,
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: "Cancel".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                },
            ],
            progress: Some(Progress {
                current_step: count as u8,
                total_steps: threshold as u8,
                label: Some(format!("{count}/{threshold} vouchers")),
            }),
            ..Default::default()
        }
    }

    fn build_complete_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_status".into(),
            title: "Recovery Complete".into(),
            subtitle: None,
            components: vec![
                Component::StatusIndicator {
                    id: "recovery_complete".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: "Recovery Proof Submitted".into(),
                    detail: Some(
                        "Your contacts will be notified. They can accept \
                         your new identity to restore your contact relationships."
                            .into(),
                    ),
                    status: Status::Success,
                    a11y: None,
                },
                Component::Text {
                    id: "what_is_recovered".into(),
                    content: "What is recovered: contact relationships \
                              and the ability to communicate with your contacts."
                        .into(),
                    style: TextStyle::Body,
                },
                Component::Text {
                    id: "what_is_not_recovered".into(),
                    content: "NOT recovered: message history, device-specific \
                              settings, and trust levels. Your contacts will \
                              re-send their cards once they accept your recovery."
                        .into(),
                    style: TextStyle::Caption,
                },
            ],
            actions: vec![ScreenAction {
                id: "done".into(),
                label: "Done".into(),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for RecoveryEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Status screen actions
            (RecoveryStep::Status, UserAction::ActionPressed { ref action_id })
                if action_id == "start_recovery" =>
            {
                self.step = RecoveryStep::ShowClaimQr;
                self.collected_vouchers.clear();
                ActionResult::UpdateScreen(self.build_screen())
            }
            (RecoveryStep::Status, UserAction::ActionPressed { ref action_id })
                if action_id == "check_status" =>
            {
                ActionResult::ShowAlert {
                    title: "Recovery Status".into(),
                    message: "No active recovery claims.".into(),
                }
            }

            // ShowClaimQr actions
            (RecoveryStep::ShowClaimQr, UserAction::ActionPressed { ref action_id })
                if action_id == "wait_for_voucher" =>
            {
                self.step = RecoveryStep::CollectVouchers;
                ActionResult::UpdateScreen(self.build_screen())
            }
            (RecoveryStep::ShowClaimQr, UserAction::ActionPressed { ref action_id })
                if action_id == "cancel" =>
            {
                self.step = RecoveryStep::Status;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // CollectVouchers actions
            (RecoveryStep::CollectVouchers, UserAction::ActionPressed { ref action_id })
                if action_id == "submit_proof" && self.threshold_met() =>
            {
                self.step = RecoveryStep::Complete;
                ActionResult::UpdateScreen(self.build_screen())
            }
            (RecoveryStep::CollectVouchers, UserAction::ActionPressed { ref action_id })
                if action_id == "cancel" =>
            {
                self.step = RecoveryStep::Status;
                self.collected_vouchers.clear();
                ActionResult::UpdateScreen(self.build_screen())
            }

            // Complete actions
            (RecoveryStep::Complete, UserAction::ActionPressed { ref action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }

            // Default: refresh current screen
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}
