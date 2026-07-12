// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Social recovery engine — outgoing recovery flow.
//!
//! State machine: Status → ShowClaimQr → CollectVouchers → Complete.
//! The recovering user shows a claim QR, collects voucher scans from
//! guardians, and submits the proof when the threshold is met.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Steps in the outgoing recovery workflow.
#[derive(Clone, Debug, PartialEq)]
enum RecoveryStep {
    /// Initial screen: "Lost your device?" intro card + recovery
    /// settings (required vouchers, claim expiry, trusted-contact
    /// readiness) + 4-step instructions + "Start Recovery" button.
    /// Reached on fresh navigation when no recovery is in progress.
    Intro,
    /// Old-public-key entry: TextInput for the lost identity's public
    /// key (hex), then a "Create Claim" action that the AppEngine
    /// intercept turns into a base64 claim payload.
    EnterOldKey,
    /// Generated claim is shown to the user so they can copy it and
    /// share with their trusted contacts. Reached after successful
    /// claim creation from EnterOldKey.
    ShowGeneratedClaim,
    /// In-progress recovery: quorum status + trusted contacts list.
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
    trusted_contacts: Vec<Item>,
    quorum_threshold: usize,
    /// Number of linked devices (0 = this is the only device).
    linked_device_count: usize,
    /// Claim data (old_pk) set before starting recovery.
    claim_data: Option<[u8; 32]>,
    /// Vouchers collected so far (display records).
    collected_vouchers: Vec<CollectedVoucher>,
    /// User-entered hex string for the old identity's public key.
    /// Updated on `TextChanged` from the EnterOldKey screen, read by
    /// the AppEngine intercept when the user presses "create_claim".
    old_key_input: String,
    /// Validation error to display on the old-key input (set by the
    /// AppEngine intercept when claim creation fails).
    old_key_error: Option<String>,
    /// Generated claim payload (base64), populated by the AppEngine
    /// intercept after `Vauchi::create_recovery_claim_hex_b64` succeeds.
    generated_claim_b64: Option<String>,
    locale: Locale,
}

impl RecoveryEngine {
    pub fn new(trusted_contacts: Vec<Item>, quorum_threshold: usize) -> Self {
        Self {
            step: RecoveryStep::Intro,
            trusted_contacts,
            quorum_threshold,
            linked_device_count: 0,
            claim_data: None,
            collected_vouchers: Vec::new(),
            old_key_input: String::new(),
            old_key_error: None,
            generated_claim_b64: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-2).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns the user-entered old public key hex (set via `TextChanged`).
    /// Read by the AppEngine intercept when handling "create_claim".
    pub fn old_key_input(&self) -> &str {
        &self.old_key_input
    }

    /// True while the user is on the EnterOldKey step — i.e. the
    /// AppEngine intercept should look at `old_key_input` to drive
    /// claim creation when "create_claim" is pressed.
    pub fn is_at_enter_old_key_step(&self) -> bool {
        self.step == RecoveryStep::EnterOldKey
    }

    /// Records the generated claim payload and advances to ShowGeneratedClaim.
    /// Called by the AppEngine intercept after a successful
    /// `Vauchi::create_recovery_claim_hex_b64`.
    pub fn set_generated_claim(&mut self, claim_b64: impl Into<String>) {
        self.generated_claim_b64 = Some(claim_b64.into());
        self.old_key_error = None;
        self.step = RecoveryStep::ShowGeneratedClaim;
    }

    /// Records a claim-creation failure and stays on the EnterOldKey
    /// step with an error attached to the input.
    pub fn set_create_claim_error(&mut self, message: impl Into<String>) {
        self.old_key_error = Some(message.into());
    }

    /// Test-only entry point: jump the engine to the in-progress
    /// `Status` step so tests for the legacy flow (Status → ShowClaimQr
    /// → CollectVouchers → Complete) can keep exercising those states
    /// without first walking through the new Intro / EnterOldKey
    /// claim-creation steps. Production code reaches Status via the
    /// new flow plus future "resume in-progress recovery" wiring.
    #[doc(hidden)]
    pub fn _jump_to_status_for_testing(&mut self) {
        self.step = RecoveryStep::Status;
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
            RecoveryStep::Intro => self.build_intro_screen(),
            RecoveryStep::EnterOldKey => self.build_enter_old_key_screen(),
            RecoveryStep::ShowGeneratedClaim => self.build_generated_claim_screen(),
            RecoveryStep::Status => self.build_status_screen(),
            RecoveryStep::ShowClaimQr => self.build_claim_qr_screen(),
            RecoveryStep::CollectVouchers => self.build_collect_screen(),
            RecoveryStep::Complete => self.build_complete_screen(),
        }
    }

    fn build_intro_screen(&self) -> ScreenModel {
        let trusted = self.trusted_contacts.len();
        let threshold = self.quorum_threshold;
        let quorum_met = trusted >= threshold;
        let trusted_detail = format!("{trusted}/{threshold}");

        let mut components = vec![
            Component::InfoPanel {
                id: "intro".into(),
                icon: Some("lifebuoy".into()),
                title: self.t("recovery.lost_device_title"),
                items: vec![InfoItem {
                    icon: None,
                    title: String::new(),
                    detail: self.t("recovery.lost_device_description"),
                }],
                a11y: None,
            },
            Component::InfoPanel {
                id: "settings".into(),
                icon: None,
                title: self.t("recovery.settings_title"),
                items: vec![
                    InfoItem {
                        icon: None,
                        title: self.t("recovery.required_vouchers_label"),
                        detail: threshold.to_string(),
                    },
                    InfoItem {
                        icon: None,
                        title: self.t("recovery.claim_expiry_label"),
                        // 7 days matches RecoveryClaim::is_expired logic
                        // (claim is valid for 7 days from creation).
                        detail: self.t("recovery.claim_expiry_days"),
                    },
                    InfoItem {
                        icon: None,
                        title: self.t("recovery.trusted_contacts_label"),
                        detail: trusted_detail,
                    },
                ],
                a11y: None,
            },
        ];

        if !quorum_met {
            components.push(Component::StatusIndicator {
                id: "low_trusted_warning".into(),
                icon: Some("warning".into()),
                title: self.t("recovery.not_enough_trusted"),
                detail: Some(get_string_with_args(
                    self.locale,
                    "recovery.mark_more_trusted",
                    &[("count", &threshold.saturating_sub(trusted).to_string())],
                )),
                status: Status::Warning,
                status_label: self.t(Status::Warning.label_key()),
                a11y: None,
            });
        }

        components.push(self.how_it_works_panel());

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("more.social_recovery"),
            subtitle: None,
            components,
            actions: vec![ScreenAction {
                id: "start_recovery_process".into(),
                label: self.t("recovery.start_process"),
                style: ActionStyle::Primary,
                enabled: quorum_met,
                a11y: Some(A11y::labeled(self.t("recovery.start_process"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn how_it_works_panel(&self) -> Component {
        Component::InfoPanel {
            id: "how_it_works".into(),
            icon: None,
            title: self.t("recovery.how_it_works_title"),
            items: vec![
                InfoItem {
                    icon: None,
                    title: format!("1. {}", self.t("recovery.step1_title")),
                    detail: self.t("recovery.step1_desc"),
                },
                InfoItem {
                    icon: None,
                    title: format!("2. {}", self.t("recovery.step2_title")),
                    detail: self.t("recovery.step2_desc"),
                },
                InfoItem {
                    icon: None,
                    title: format!("3. {}", self.t("recovery.step3_title")),
                    detail: self.t("recovery.step3_desc_original"),
                },
                InfoItem {
                    icon: None,
                    title: format!("4. {}", self.t("recovery.step4_title")),
                    detail: self.t("recovery.step4_desc"),
                },
            ],
            a11y: None,
        }
    }

    fn build_enter_old_key_screen(&self) -> ScreenModel {
        let components = vec![
            Component::Text {
                id: "instructions".into(),
                content: self.t("recovery.enter_old_key_full_instruction"),
                style: TextStyle::Body,
            },
            Component::TextInput {
                id: "old_public_key".into(),
                label: self.t("recovery.old_public_key"),
                value: self.old_key_input.clone(),
                placeholder: Some(self.t("recovery.hex_placeholder_short")),
                max_length: Some(64),
                validation_error: self.old_key_error.clone(),
                input_type: InputType::Text,
                a11y: Some(A11y::labeled(self.t("recovery.old_public_key"))),
                info_key: None,
            },
        ];

        // Match Android's UX: Create Claim enabled once the user has
        // typed a 64-character hex string. Real validation (hex parse +
        // 32-byte length) happens in the AppEngine intercept.
        let create_enabled = self.old_key_input.trim().len() >= 64;

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("recovery.create_claim_title"),
            subtitle: None,
            components,
            actions: vec![
                ScreenAction {
                    id: "create_claim".into(),
                    label: self.t("recovery.create_claim_button"),
                    style: ActionStyle::Primary,
                    enabled: create_enabled,
                    a11y: Some(A11y::labeled(self.t("recovery.create_claim_button"))),
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

    fn build_generated_claim_screen(&self) -> ScreenModel {
        let claim = self
            .generated_claim_b64
            .as_deref()
            .unwrap_or("(claim unavailable)");

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("recovery.claim_created_title"),
            subtitle: None,
            components: vec![
                Component::StatusIndicator {
                    id: "claim_ready".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: self.t("recovery.share_with_trusted_title"),
                    detail: Some(self.t("recovery.give_claim_instruction")),
                    status: Status::Success,
                    status_label: self.t(Status::Success.label_key()),
                    a11y: None,
                },
                Component::Text {
                    id: "claim_data".into(),
                    content: claim.into(),
                    style: TextStyle::Caption,
                },
            ],
            actions: vec![
                ScreenAction {
                    id: "copy_claim".into(),
                    label: self.t("recovery.copy_claim_data"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.copy_claim_data"))),
                },
                ScreenAction {
                    id: "done".into(),
                    label: self.t("action.done"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("action.done"))),
                },
            ],
            progress: None,
            ..Default::default()
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
            actions: vec![
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

    fn build_claim_qr_screen(&self) -> ScreenModel {
        let qr_data = self.claim_data.map(hex::encode).unwrap_or_default();

        ScreenModel {
            screen_id: "recovery_status".into(),
            title: self.t("recovery.claim_label"),
            subtitle: Some(self.t("recovery.show_qr_subtitle")),
            components: vec![
                Component::QrCode {
                    id: "claim_qr".into(),
                    data: qr_data,
                    mode: QrMode::Display,
                    label: Some(self.t("recovery.claim_qr_label")),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some(self.t("recovery.claim_qr_a11y_label")),
                        hint: Some(self.t("recovery.claim_qr_a11y_hint")),
                        role: None,
                    }),
                },
                Component::Text {
                    id: "claim_instructions".into(),
                    content: self.t("recovery.claim_meet_instruction"),
                    style: TextStyle::Body,
                },
            ],
            actions: vec![
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

        // Show collected voucher names
        if !self.collected_vouchers.is_empty() {
            let vouched_label = self.t("recovery.vouched_label");
            let items: Vec<ActionListItem> = self
                .collected_vouchers
                .iter()
                .enumerate()
                .map(|(i, v)| ActionListItem {
                    id: format!("voucher_{i}"),
                    label: v.guardian_name.clone(),
                    icon: Some("checkmark.circle".into()),
                    detail: Some(vouched_label.clone()),
                    a11y: Some(A11y::labeled(v.guardian_name.clone())),
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
            actions: vec![
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

    fn build_complete_screen(&self) -> ScreenModel {
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
                    id: "what_is_recovered".into(),
                    content: self.t("recovery.what_is_recovered"),
                    style: TextStyle::Body,
                },
                Component::Text {
                    id: "what_is_not_recovered".into(),
                    content: self.t("recovery.what_is_not_recovered"),
                    style: TextStyle::Caption,
                },
            ],
            actions: vec![ScreenAction {
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

impl WorkflowEngine for RecoveryEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::Recovery {
            old_key_input: self.old_key_input().to_string(),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::Recovery(update) = update else {
            return false;
        };
        match update {
            crate::ui::RecoveryUpdate::ClaimGenerated(claim) => self.set_generated_claim(claim),
            crate::ui::RecoveryUpdate::ClaimCreateError(message) => {
                self.set_create_claim_error(message)
            }
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Intro → EnterOldKey
            (RecoveryStep::Intro, UserAction::ActionPressed { ref action_id })
                if action_id == "start_recovery_process" =>
            {
                self.step = RecoveryStep::EnterOldKey;
                self.old_key_input.clear();
                self.old_key_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // EnterOldKey: text input updates
            (
                RecoveryStep::EnterOldKey,
                UserAction::TextChanged {
                    ref component_id,
                    ref value,
                },
            ) if component_id == "old_public_key" => {
                self.old_key_input = value.clone();
                // Clear validation error as soon as the user edits.
                self.old_key_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // EnterOldKey: create_claim → outer AppEngine intercept does
            // the hex parse + Vauchi::create_recovery_claim_hex_b64 call,
            // then either set_generated_claim (advances to ShowGeneratedClaim)
            // or set_create_claim_error (stays on screen with error).
            (RecoveryStep::EnterOldKey, UserAction::ActionPressed { ref action_id })
                if action_id == "create_claim" && self.old_key_input.trim().len() >= 64 =>
            {
                ActionResult::Complete
            }

            // EnterOldKey cancel → back to Intro
            (RecoveryStep::EnterOldKey, UserAction::ActionPressed { ref action_id })
                if action_id == "cancel" =>
            {
                self.step = RecoveryStep::Intro;
                self.old_key_input.clear();
                self.old_key_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // ShowGeneratedClaim: copy is informational only — frontend
            // handles the clipboard write. Done navigates back to Intro
            // so the user can start another recovery if needed.
            (RecoveryStep::ShowGeneratedClaim, UserAction::ActionPressed { ref action_id })
                if action_id == "copy_claim" =>
            {
                ActionResult::UpdateScreen(self.build_screen())
            }
            (RecoveryStep::ShowGeneratedClaim, UserAction::ActionPressed { ref action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }

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
                    title: self.t("recovery.status"),
                    message: self.t("recovery.no_active_claims_alert"),
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

// INLINE_TEST_REQUIRED: tests assert engine state machine across the new
// Intro → EnterOldKey → ShowGeneratedClaim transitions and validate the
// setter methods that the AppEngine intercept depends on. Extracted to
// recovery_status_tests.rs to keep this file under the 1000-line src
// hard limit (M3 S5-2).
#[cfg(test)]
#[path = "recovery_status_tests.rs"]
mod tests;
