// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery claim review engine — incoming recovery flows.
//!
//! Two modes:
//! - **Vouching**: user is a guardian, reviews the claim and creates a voucher.
//! - **Acceptance**: user reviews a completed proof and accepts the new identity.
//!
//! Confidence levels drive the UI:
//! - High: mutual contacts vouched → safe, one-tap accept/vouch.
//! - Medium: some mutual contacts → warning, suggest verifying.
//! - Low: no mutual contacts → risky, require out-of-band verification.

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::*;

/// Whether the user is vouching or reviewing a completed proof.
#[derive(Clone, Debug, PartialEq)]
pub enum ReviewMode {
    /// User is a designated guardian creating a voucher.
    Vouching,
    /// User is reviewing a completed recovery proof.
    Acceptance,
}

/// Confidence level for the recovery claim.
#[derive(Clone, Debug, PartialEq)]
pub enum Confidence {
    /// Multiple mutual contacts vouched — safe.
    High,
    /// Some mutual contacts — proceed with caution.
    Medium,
    /// No mutual contacts — verify out-of-band.
    Low,
}

/// Context about the recovery claim being reviewed.
#[derive(Clone, Debug)]
pub struct ClaimContext {
    /// Display name of the contact who is recovering.
    pub contact_name: String,
    /// Fingerprint of the old public key (for out-of-band verification).
    pub old_pk_fingerprint: String,
    /// Number of mutual contacts who already vouched.
    pub mutual_voucher_count: usize,
    /// Quorum threshold.
    pub threshold: usize,
    /// Computed confidence level.
    pub confidence: Confidence,
}

/// Steps in the claim review workflow.
#[derive(Clone, Debug, PartialEq)]
enum ReviewStep {
    /// Initial review screen — shows confidence and actions.
    Review,
    /// Out-of-band verification — shows old_pk fingerprint.
    VerifyOutOfBand,
    /// Confirm risky acceptance (low confidence).
    ConfirmAccept,
    /// Voucher QR displayed (vouching mode only).
    ShowVoucherQr,
    /// Terminal: user rejected or reminded.
    Done,
}

/// Engine for reviewing incoming recovery claims.
///
/// ADR-021 compliant: all UI via ScreenModel. Frontends render
/// components and forward UserActions.
#[derive(Clone, Debug)]
pub struct RecoveryClaimReviewEngine {
    mode: ReviewMode,
    context: ClaimContext,
    step: ReviewStep,
    cancelled: bool,
    locale: Locale,
}

impl RecoveryClaimReviewEngine {
    pub fn new(mode: ReviewMode, context: ClaimContext) -> Self {
        Self {
            mode,
            context,
            step: ReviewStep::Review,
            cancelled: false,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-3).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            ReviewStep::Review => self.build_review_screen(),
            ReviewStep::VerifyOutOfBand => self.build_verify_screen(),
            ReviewStep::ConfirmAccept => self.build_confirm_screen(),
            ReviewStep::ShowVoucherQr => self.build_voucher_qr_screen(),
            ReviewStep::Done => self.build_done_screen(),
        }
    }

    fn build_review_screen(&self) -> ScreenModel {
        let (status, status_icon, detail) = match self.context.confidence {
            Confidence::High => (
                Status::Success,
                "checkmark.shield",
                get_string_with_args(
                    self.locale,
                    "recovery.confidence_high_detail",
                    &[("count", &self.context.mutual_voucher_count.to_string())],
                ),
            ),
            Confidence::Medium => (
                Status::Warning,
                "exclamationmark.triangle",
                get_string_with_args(
                    self.locale,
                    "recovery.confidence_medium_detail",
                    &[("count", &self.context.mutual_voucher_count.to_string())],
                ),
            ),
            Confidence::Low => (
                Status::Pending,
                "questionmark.circle",
                self.t("recovery.confidence_low_detail"),
            ),
        };

        let actions = self.build_actions();

        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: get_string_with_args(
                self.locale,
                "recovery.review_title",
                &[("name", &self.context.contact_name)],
            ),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "confidence".into(),
                icon: Some(status_icon.into()),
                title: self.t("recovery.verification_confidence"),
                detail: Some(detail),
                status,
                status_label: self.t(status.label_key()),
                a11y: None,
            }],
            contextual_actions: actions,
            progress: None,
            ..Default::default()
        }
    }

    fn build_actions(&self) -> Vec<ScreenAction> {
        match &self.context.confidence {
            Confidence::High => self.actions_high(),
            Confidence::Medium => self.actions_medium(),
            Confidence::Low => self.actions_low(),
        }
    }

    fn actions_high(&self) -> Vec<ScreenAction> {
        let mut actions = Vec::new();
        match &self.mode {
            ReviewMode::Vouching => {
                actions.push(ScreenAction {
                    id: "vouch".into(),
                    label: self.t("recovery.vouch_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.vouch_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "accept".into(),
                    label: self.t("recovery.accept_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.accept_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
        }
        actions
    }

    fn actions_medium(&self) -> Vec<ScreenAction> {
        let mut actions = Vec::new();
        match &self.mode {
            ReviewMode::Vouching => {
                actions.push(ScreenAction {
                    id: "vouch".into(),
                    label: self.t("recovery.vouch_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.vouch_button"))),
                });
                actions.push(ScreenAction {
                    id: "remind".into(),
                    label: self.t("recovery.remind_later_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.remind_later_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "accept".into(),
                    label: self.t("recovery.accept_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.accept_button"))),
                });
                actions.push(ScreenAction {
                    id: "remind".into(),
                    label: self.t("recovery.remind_later_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.remind_later_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
        }
        actions
    }

    fn actions_low(&self) -> Vec<ScreenAction> {
        let mut actions = Vec::new();
        match &self.mode {
            ReviewMode::Vouching => {
                actions.push(ScreenAction {
                    id: "verify_other".into(),
                    label: self.t("recovery.verify_another_way_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.verify_another_way_button"))),
                });
                actions.push(ScreenAction {
                    id: "vouch".into(),
                    label: self.t("recovery.vouch_anyway_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.vouch_anyway_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "verify_other".into(),
                    label: self.t("recovery.verify_another_way_button"),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.verify_another_way_button"))),
                });
                actions.push(ScreenAction {
                    id: "accept_anyway".into(),
                    label: self.t("recovery.accept_anyway_button"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.accept_anyway_button"))),
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: self.t("device_link.reject"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("device_link.reject"))),
                });
            }
        }
        actions
    }

    fn build_verify_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: self.t("recovery.vouch_step1_title"),
            subtitle: Some(self.t("recovery.verify_screen_subtitle")),
            components: vec![
                Component::Text {
                    id: "fingerprint_label".into(),
                    content: self.t("recovery.old_fingerprint_label"),
                    style: TextStyle::Caption,
                },
                Component::Text {
                    id: "fingerprint".into(),
                    content: self.context.old_pk_fingerprint.clone(),
                    style: TextStyle::Title,
                },
                Component::Text {
                    id: "instructions".into(),
                    content: self.t("recovery.verify_call_instruction"),
                    style: TextStyle::Body,
                },
            ],
            contextual_actions: vec![ScreenAction {
                id: "back".into(),
                label: self.t("action.back"),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("action.back"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: self.t("recovery.confirm_acceptance_title"),
            subtitle: None,
            components: vec![Component::InlineConfirm {
                id: "risky_accept".into(),
                warning: get_string_with_args(
                    self.locale,
                    "recovery.risky_accept_warning",
                    &[("name", &self.context.contact_name)],
                ),
                confirm_text: self.t("recovery.accept_anyway_button"),
                cancel_text: self.t("action.cancel"),
                confirm_action_id: "confirm_accept".into(),
                cancel_action_id: "cancel".into(),
                destructive: true,
                a11y: None,
            }],
            contextual_actions: vec![
                ScreenAction {
                    id: "confirm_accept".into(),
                    label: self.t("recovery.accept_anyway_button"),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.accept_anyway_button"))),
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

    fn build_voucher_qr_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: self.t("recovery.voucher_created_title"),
            subtitle: Some(self.t("recovery.voucher_created_subtitle")),
            components: vec![
                Component::QrCode {
                    id: "voucher_qr".into(),
                    data: "voucher-placeholder".into(),
                    frames: Vec::new(),
                    mode: QrMode::Display,
                    label: Some(self.t("recovery.voucher_qr_label")),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some(self.t("recovery.voucher_qr_a11y_label")),
                        hint: Some(self.t("recovery.voucher_qr_a11y_hint")),
                        role: None,
                    }),
                },
                Component::StatusIndicator {
                    id: "voucher_status".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: self.t("recovery.voucher_signed_title"),
                    detail: Some(get_string_with_args(
                        self.locale,
                        "recovery.voucher_signed_detail",
                        &[("name", &self.context.contact_name)],
                    )),
                    status: Status::Success,
                    status_label: self.t(Status::Success.label_key()),
                    a11y: None,
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

    fn build_done_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: self.t("action.done"),
            subtitle: None,
            components: vec![],
            contextual_actions: vec![],
            progress: None,
            ..Default::default()
        }
    }
}

impl WorkflowEngine for RecoveryClaimReviewEngine {
    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Review screen actions
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "vouch" =>
            {
                self.step = ReviewStep::ShowVoucherQr;
                ActionResult::UpdateScreen(self.build_screen())
            }
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "accept" =>
            {
                self.step = ReviewStep::Done;
                ActionResult::Complete
            }
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "reject" =>
            {
                self.cancelled = true;
                self.step = ReviewStep::Done;
                ActionResult::Complete
            }
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "remind" =>
            {
                self.step = ReviewStep::Done;
                ActionResult::Complete
            }
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "verify_other" =>
            {
                self.step = ReviewStep::VerifyOutOfBand;
                ActionResult::UpdateScreen(self.build_screen())
            }
            (ReviewStep::Review, UserAction::ActionPressed { ref action_id })
                if action_id == "accept_anyway" =>
            {
                self.step = ReviewStep::ConfirmAccept;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // Verify out-of-band
            (ReviewStep::VerifyOutOfBand, UserAction::ActionPressed { ref action_id })
                if action_id == "back" =>
            {
                self.step = ReviewStep::Review;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // Confirm risky accept
            (ReviewStep::ConfirmAccept, UserAction::ActionPressed { ref action_id })
                if action_id == "confirm_accept" =>
            {
                self.step = ReviewStep::Done;
                ActionResult::Complete
            }
            (ReviewStep::ConfirmAccept, UserAction::ActionPressed { ref action_id })
                if action_id == "cancel" =>
            {
                self.step = ReviewStep::Review;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // Voucher QR done
            (ReviewStep::ShowVoucherQr, UserAction::ActionPressed { ref action_id })
                if action_id == "done" =>
            {
                self.step = ReviewStep::Done;
                ActionResult::Complete
            }

            // Default
            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }

    fn was_cancelled(&self) -> bool {
        self.cancelled
    }
}
