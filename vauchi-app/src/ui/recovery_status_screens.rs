// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The recovery engine's screens once a recovery is under way: its
//! status, the claim QR, collecting vouchers, and the completed proof.
//! Split out of `recovery_status.rs` to keep it under the 1000-line limit.

use super::*;

impl RecoveryEngine {
    pub(super) fn build_status_screen(&self) -> ScreenModel {
        let current = self.trusted_contacts.len();
        let quorum_met = current >= self.quorum_threshold;

        let mut components = Vec::new();

        // Multi-device awareness: if user has other devices, suggest
        // revoking from a surviving device instead of full recovery.
        if self.linked_device_count > 0 {
            components.push(Component::StatusIndicator {
                id: "multi_device_hint".into(),
                icon: Some("link".into()),
                title: self.t("recovery.linked_devices_available"),
                detail: Some(get_string_with_args(
                    self.locale,
                    "recovery.linked_devices_hint",
                    &[("count", &self.linked_device_count.to_string())],
                )),
                status: Status::Success,
                status_label: self.t(Status::Success.label_key()),
                a11y: None,
            });
        }

        components.push(Component::InfoPanel {
            id: "quorum_info".into(),
            icon: Some("lifebuoy".into()),
            title: self.t("recovery.quorum_status_title"),
            items: vec![
                InfoItem {
                    icon: None,
                    title: self.t("resistance.emergency.trusted_contacts"),
                    detail: get_string_with_args(
                        self.locale,
                        "recovery.contacts_of_threshold",
                        &[
                            ("current", &current.to_string()),
                            ("threshold", &self.quorum_threshold.to_string()),
                        ],
                    ),
                },
                InfoItem {
                    icon: None,
                    title: self.t("recovery.quorum_met_label"),
                    detail: if quorum_met {
                        self.t("generic.yes")
                    } else {
                        self.t("generic.no")
                    },
                },
            ],
            a11y: None,
        });

        components.push(Component::List {
            id: "trusted_contacts".into(),
            items: self.trusted_contacts.clone(),
            searchable: false,
            total_count: 0,
            offset: 0,
            window: 0,
        });

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("more.social_recovery"),
            subtitle: None,
            components,
            contextual_actions: vec![
                ScreenAction {
                    id: "start_recovery".into(),
                    label: self.t("recovery.start_recovery_short"),
                    style: ActionStyle::Primary,
                    enabled: quorum_met,
                    a11y: Some(A11y::labeled(self.t("recovery.start_recovery_short"))),
                },
                ScreenAction {
                    id: "check_status".into(),
                    label: self.t("recovery.check_status_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.check_status_button"))),
                },
            ],
            progress: None,
            ..Default::default()
        }
    }

    pub(super) fn build_claim_qr_screen(&self) -> ScreenModel {
        let qr_data = self.claim_data.map(hex::encode).unwrap_or_default();

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("recovery.claim_label"),
            subtitle: Some(self.t("recovery.show_qr_subtitle")),
            components: vec![
                Component::QrCode {
                    id: "claim_qr".into(),
                    data: qr_data,
                    frames: Vec::new(),
                    mode: QrMode::Display,
                    label: Some(self.t("recovery.claim_qr_label")),
                    scan_quality: None,
                    placement: None,
                    error_correction: None,
                    size: None,
                    a11y: Some(A11y {
                        label: Some(self.t("recovery.claim_qr_a11y_label")),
                        hint: Some(self.t("recovery.claim_qr_a11y_hint")),
                        role: None,
                    }),
                },
                Component::Text {
                    a11y: None,
                    id: "claim_instructions".into(),
                    content: self.t("recovery.claim_meet_instruction"),
                    style: TextStyle::Body,
                },
            ],
            contextual_actions: vec![
                ScreenAction {
                    id: "wait_for_voucher".into(),
                    label: self.t("recovery.step3_title"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.step3_title"))),
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

    pub(super) fn build_collect_screen(&self) -> ScreenModel {
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
            icon: Some("lifebuoy".into()),
            title: self.t("recovery.voucher_collection_title"),
            detail: Some(get_string_with_args(
                self.locale,
                "recovery.vouchers_collected_count",
                &[
                    ("count", &count.to_string()),
                    ("threshold", &threshold.to_string()),
                ],
            )),
            status,
            status_label: self.t(status.label_key()),
            a11y: None,
        }];

        components.push(Component::TextInput {
            id: "voucher_input".into(),
            label: self.t("recovery.paste_voucher_label"),
            value: self.voucher_input.clone(),
            placeholder: Some(self.t("recovery.paste_voucher_placeholder")),
            max_length: None,
            validation_error: self.voucher_error.clone(),
            input_type: InputType::Text,
            a11y: Some(A11y::labeled(self.t("recovery.paste_voucher_label"))),
            info_key: None,
        });

        // Show collected voucher names
        if !self.collected_vouchers.is_empty() {
            let vouched_label = self.t("recovery.vouched_label");
            let items: Vec<ActionListItem> = self
                .collected_vouchers
                .iter()
                .enumerate()
                .map(|(i, v)| ActionListItem {
                    id: format!("voucher_{i}"),
                    label: v.signer_label.clone(),
                    icon: Some("checkmark.circle".into()),
                    detail: Some(vouched_label.clone()),
                    a11y: Some(A11y::labeled(v.signer_label.clone())),
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
            title: self.t("recovery.collecting_vouchers_title"),
            subtitle: None,
            components,
            contextual_actions: vec![
                ScreenAction {
                    id: "add_voucher".into(),
                    label: self.t("recovery.add_voucher"),
                    style: ActionStyle::Secondary,
                    enabled: !self.voucher_input.trim().is_empty(),
                    a11y: Some(A11y::labeled(self.t("recovery.add_voucher"))),
                },
                ScreenAction {
                    id: "submit_proof".into(),
                    label: self.t("recovery.submit_proof_button"),
                    style: ActionStyle::Primary,
                    enabled: met,
                    a11y: Some(A11y::labeled(self.t("recovery.submit_proof_button"))),
                },
                ScreenAction {
                    id: "cancel".into(),
                    label: self.t("action.cancel"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.cancel"))),
                },
            ],
            progress: Some(Progress {
                current_step: count as u8,
                total_steps: threshold as u8,
                label: Some(get_string_with_args(
                    self.locale,
                    "recovery.vouchers_progress_label",
                    &[
                        ("count", &count.to_string()),
                        ("threshold", &threshold.to_string()),
                    ],
                )),
            }),
            ..Default::default()
        }
    }

    pub(super) fn build_complete_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("recovery.complete_title"),
            subtitle: None,
            components: vec![
                Component::StatusIndicator {
                    id: "recovery_complete".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: self.t("recovery.proof_submitted_title"),
                    detail: Some(self.t("recovery.proof_submitted_detail")),
                    status: Status::Success,
                    status_label: self.t(Status::Success.label_key()),
                    a11y: None,
                },
                Component::Text {
                    a11y: None,
                    id: "what_is_recovered".into(),
                    content: self.t("recovery.what_is_recovered"),
                    style: TextStyle::Body,
                },
                Component::Text {
                    a11y: None,
                    id: "what_is_not_recovered".into(),
                    content: self.t("recovery.what_is_not_recovered"),
                    style: TextStyle::Caption,
                },
            ],
            contextual_actions: vec![ScreenAction {
                id: "done".into(),
                label: self.t("action.done"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.done"))),
            }],
            progress: None,
            ..Default::default()
        }
    }
}
