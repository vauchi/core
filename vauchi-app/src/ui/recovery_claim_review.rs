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
}

impl RecoveryClaimReviewEngine {
    pub fn new(mode: ReviewMode, context: ClaimContext) -> Self {
        Self {
            mode,
            context,
            step: ReviewStep::Review,
            cancelled: false,
        }
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
                format!(
                    "{} mutual contacts vouched. This looks safe.",
                    self.context.mutual_voucher_count
                ),
            ),
            Confidence::Medium => (
                Status::Warning,
                "exclamationmark.triangle",
                format!(
                    "{} mutual contact(s) vouched. Consider verifying before proceeding.",
                    self.context.mutual_voucher_count
                ),
            ),
            Confidence::Low => (
                Status::Pending,
                "questionmark.circle",
                "No mutual contacts vouched. Verify this person's identity through \
                 another channel before proceeding."
                    .into(),
            ),
        };

        let actions = self.build_actions();

        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: format!("Recovery: {}", self.context.contact_name),
            subtitle: None,
            components: vec![Component::StatusIndicator {
                id: "confidence".into(),
                icon: Some(status_icon.into()),
                title: "Verification Confidence".into(),
                detail: Some(detail),
                status,
                a11y: None,
            }],
            actions,
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
                    label: "Vouch".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "accept".into(),
                    label: "Accept".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
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
                    label: "Vouch".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "remind".into(),
                    label: "Remind Me Later".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "accept".into(),
                    label: "Accept".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "remind".into(),
                    label: "Remind Me Later".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
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
                    label: "Verify Another Way".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "vouch".into(),
                    label: "Vouch Anyway".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                });
            }
            ReviewMode::Acceptance => {
                actions.push(ScreenAction {
                    id: "verify_other".into(),
                    label: "Verify Another Way".into(),
                    style: ActionStyle::Primary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "accept_anyway".into(),
                    label: "Accept Anyway".into(),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: None,
                });
                actions.push(ScreenAction {
                    id: "reject".into(),
                    label: "Reject".into(),
                    style: ActionStyle::Destructive,
                    enabled: true,
                    a11y: None,
                });
            }
        }
        actions
    }

    fn build_verify_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: "Verify Identity".into(),
            subtitle: Some("Compare this fingerprint with the contact via phone or text".into()),
            components: vec![
                Component::Text {
                    id: "fingerprint_label".into(),
                    content: "Old identity fingerprint:".into(),
                    style: TextStyle::Caption,
                },
                Component::Text {
                    id: "fingerprint".into(),
                    content: self.context.old_pk_fingerprint.clone(),
                    style: TextStyle::Title,
                },
                Component::Text {
                    id: "instructions".into(),
                    content: "Ask the person to read their old fingerprint aloud \
                              over a phone call. If it matches, you can safely vouch."
                        .into(),
                    style: TextStyle::Body,
                },
            ],
            actions: vec![ScreenAction {
                id: "back".into(),
                label: "Back".into(),
                style: ActionStyle::Secondary,
                enabled: true,
                a11y: None,
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn build_confirm_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: "Confirm Acceptance".into(),
            subtitle: None,
            components: vec![Component::InlineConfirm {
                id: "risky_accept".into(),
                warning: format!(
                    "No mutual contacts have vouched for {}. \
                     Accepting without verification is risky — \
                     this could be an impersonation attempt.",
                    self.context.contact_name
                ),
                confirm_text: "Accept Anyway".into(),
                cancel_text: "Cancel".into(),
                destructive: true,
                a11y: None,
            }],
            actions: vec![
                ScreenAction {
                    id: "confirm_accept".into(),
                    label: "Accept Anyway".into(),
                    style: ActionStyle::Destructive,
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

    fn build_voucher_qr_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: "Voucher Created".into(),
            subtitle: Some(
                "Show this QR code to the recovering contact so they can scan it".into(),
            ),
            components: vec![
                Component::QrCode {
                    id: "voucher_qr".into(),
                    data: "voucher-placeholder".into(),
                    mode: QrMode::Display,
                    label: Some("Recovery voucher".into()),
                    scan_quality: None,
                    a11y: Some(A11y {
                        label: Some("Recovery voucher QR code".into()),
                        hint: Some("The recovering contact scans this to add your voucher".into()),
                        role: None,
                    }),
                },
                Component::StatusIndicator {
                    id: "voucher_status".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: "Voucher Signed".into(),
                    detail: Some(format!(
                        "You vouched for {}'s recovery.",
                        self.context.contact_name
                    )),
                    status: Status::Success,
                    a11y: None,
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

    fn build_done_screen(&self) -> ScreenModel {
        ScreenModel {
            screen_id: "recovery_claim_review".into(),
            title: "Done".into(),
            subtitle: None,
            components: vec![],
            actions: vec![],
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
