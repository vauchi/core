// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Recovery help engine — incoming (helper) side of the social recovery flow.
//!
//! Drives the "Help Others" screen that lets the user vouch for a contact
//! who has lost their device. State machine:
//!   Info → PasteClaim → ConfirmVoucher → ShowVoucher
//!
//! The engine renders each step but defers the actual crypto operations
//! (parse claim, create voucher) to the outer `AppEngine` intercept,
//! which has `Vauchi` access. The intercept reads the current engine
//! state to know what to do, then writes the result back via
//! `set_parsed_claim` / `set_voucher_data` and advances the step.

use crate::i18n::{Locale, get_string};
use crate::ui::*;
use vauchi_core::recovery::RECOVERY_CLAIM_MIN_INPUT_LEN;

/// Steps in the helper-side recovery workflow.
#[derive(Clone, Debug, PartialEq, Eq)]
enum HelpStep {
    /// Static info screen with steps + "Vouch for Someone" button.
    Info,
    /// Paste-claim step: TextInput for the base64 claim payload.
    PasteClaim,
    /// Show parsed claim info + "Create Voucher" button. Reached after
    /// the AppEngine intercept successfully parses the pasted claim.
    ConfirmVoucher,
    /// Show the generated voucher data + "Done" button. Reached after
    /// the AppEngine intercept creates the voucher from the confirmed claim.
    ShowVoucher,
}

/// Display-only summary of a parsed `RecoveryClaim`.
///
/// Hex-encoded keys keep this serializable across FFI without exposing
/// raw byte arrays. The frontend only needs the prefix for display.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedClaimSummary {
    pub old_pk_hex: String,
    pub new_pk_hex: String,
    pub is_expired: bool,
}

/// Engine that drives the helper-side social recovery flow.
#[derive(Clone, Debug)]
pub struct RecoveryHelpEngine {
    step: HelpStep,
    /// Pasted claim payload (base64). Updated on `TextChanged`.
    claim_input: String,
    /// Validation error to display on the claim input (set by AppEngine
    /// intercept when parse fails).
    claim_error: Option<String>,
    /// Parsed claim summary, populated by the AppEngine intercept after
    /// a successful parse.
    parsed_claim: Option<ParsedClaimSummary>,
    /// Generated voucher payload (base64), populated by the AppEngine
    /// intercept after voucher creation.
    voucher_data: Option<String>,
    locale: Locale,
}

impl Default for RecoveryHelpEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl RecoveryHelpEngine {
    pub fn new() -> Self {
        Self {
            step: HelpStep::Info,
            claim_input: String::new(),
            claim_error: None,
            parsed_claim: None,
            voucher_data: None,
            locale: Locale::English,
        }
    }

    /// Set the render locale (defaults to English) — threaded from the
    /// frontend-pushed RenderContext at the AppEngine factory (M3 S5-8).
    pub fn with_locale(mut self, locale: Locale) -> Self {
        self.locale = locale;
        self
    }

    fn t(&self, key: &str) -> String {
        get_string(self.locale, key)
    }

    /// Returns the current claim input value (set by user via `TextChanged`).
    /// The AppEngine intercept reads this when handling the "verify_claim" action.
    pub fn claim_input(&self) -> &str {
        &self.claim_input
    }

    /// Returns true while the engine is awaiting the claim parse — i.e. the
    /// user just pressed "verify_claim" from the PasteClaim step.
    pub fn is_at_paste_claim_step(&self) -> bool {
        self.step == HelpStep::PasteClaim
    }

    /// Returns true while the engine is awaiting voucher creation — i.e. the
    /// user just pressed "create_voucher" from the ConfirmVoucher step.
    pub fn is_at_confirm_voucher_step(&self) -> bool {
        self.step == HelpStep::ConfirmVoucher
    }

    /// Returns the parsed claim summary, if the user is at or past the
    /// ConfirmVoucher step. Used by the AppEngine intercept when creating
    /// the voucher.
    pub fn parsed_claim(&self) -> Option<&ParsedClaimSummary> {
        self.parsed_claim.as_ref()
    }

    /// Records a successfully parsed claim and advances to ConfirmVoucher.
    /// Called by the AppEngine intercept.
    pub fn set_parsed_claim(&mut self, claim: ParsedClaimSummary) {
        self.parsed_claim = Some(claim);
        self.claim_error = None;
        self.step = HelpStep::ConfirmVoucher;
    }

    /// Records a parse failure and stays on the PasteClaim step with an
    /// error message attached to the input.
    pub fn set_claim_parse_error(&mut self, message: impl Into<String>) {
        self.claim_error = Some(message.into());
    }

    /// Records the generated voucher data and advances to ShowVoucher.
    /// Called by the AppEngine intercept.
    pub fn set_voucher_data(&mut self, voucher_data: impl Into<String>) {
        self.voucher_data = Some(voucher_data.into());
        self.step = HelpStep::ShowVoucher;
    }

    fn build_screen(&self) -> ScreenModel {
        match &self.step {
            HelpStep::Info => self.build_info_screen(),
            HelpStep::PasteClaim => self.build_paste_claim_screen(),
            HelpStep::ConfirmVoucher => self.build_confirm_voucher_screen(),
            HelpStep::ShowVoucher => self.build_show_voucher_screen(),
        }
    }

    fn build_info_screen(&self) -> ScreenModel {
        let components = vec![
            Component::InfoPanel {
                id: "help_intro".into(),
                icon: Some("lifebuoy".into()),
                title: self.t("recovery.help_contact_recover"),
                items: vec![InfoItem {
                    icon: None,
                    title: String::new(),
                    detail: self.t("recovery.help_intro_detail"),
                }],
                a11y: None,
            },
            Component::StatusIndicator {
                id: "verify_warning".into(),
                icon: Some("warning".into()),
                title: self.t("recovery.verify_in_person_title"),
                detail: Some(self.t("recovery.vouch_warning")),
                status: Status::Warning,
                status_label: self.t(Status::Warning.label_key()),
                a11y: None,
            },
            Component::InfoPanel {
                id: "help_steps".into(),
                icon: None,
                title: self.t("recovery.how_to_vouch"),
                items: vec![
                    InfoItem {
                        icon: None,
                        title: format!("1. {}", self.t("recovery.vouch_step1_title")),
                        detail: self.t("recovery.vouch_step1_desc"),
                    },
                    InfoItem {
                        icon: None,
                        title: format!("2. {}", self.t("recovery.vouch_step2_title")),
                        detail: self.t("recovery.vouch_step2_desc_alt"),
                    },
                    InfoItem {
                        icon: None,
                        title: format!("3. {}", self.t("recovery.vouch_step3_title")),
                        detail: self.t("recovery.vouch_step3_desc"),
                    },
                    InfoItem {
                        icon: None,
                        title: format!("4. {}", self.t("recovery.vouch_step4_title")),
                        detail: self.t("recovery.vouch_step4_desc"),
                    },
                ],
                a11y: None,
            },
        ];

        ScreenModel {
            screen_id: "recovery_help".into(),
            title: self.t("recovery.tab_help_others"),
            subtitle: None,
            components,
            contextual_actions: vec![ScreenAction {
                id: "vouch".into(),
                label: self.t("recovery.vouch_for_someone"),
                style: ActionStyle::Primary,
                enabled: true,
                a11y: Some(A11y::labeled(self.t("recovery.vouch_for_someone"))),
            }],
            progress: None,
            ..Default::default()
        }
    }

    fn build_paste_claim_screen(&self) -> ScreenModel {
        let components = vec![
            Component::Text {
                id: "paste_instructions".into(),
                content: self.t("recovery.paste_instructions"),
                style: TextStyle::Body,
            },
            Component::TextInput {
                id: "claim_data".into(),
                label: self.t("recovery.claim_data_label"),
                value: self.claim_input.clone(),
                placeholder: Some(self.t("recovery.paste_claim_placeholder")),
                max_length: None,
                validation_error: self.claim_error.clone(),
                input_type: InputType::Text,
                a11y: Some(A11y::labeled(self.t("recovery.claim_data_label"))),
                info_key: None,
            },
        ];

        // Verify button enabled once the input has enough bytes to
        // plausibly be a base64-encoded claim. The actual parse happens
        // in the AppEngine intercept. Frontends (iOS RecoveryView,
        // Android RecoveryScreen) source the same threshold via
        // `recovery_claim_min_input_length()` UniFFI export.
        let verify_enabled = self.claim_input.trim().len() >= RECOVERY_CLAIM_MIN_INPUT_LEN;

        ScreenModel {
            screen_id: "recovery_help".into(),
            title: self.t("recovery.vouch_for_recovery"),
            subtitle: None,
            components,
            contextual_actions: vec![
                ScreenAction {
                    id: "verify_claim".into(),
                    label: self.t("recovery.verify_button"),
                    style: ActionStyle::Primary,
                    enabled: verify_enabled,
                    a11y: Some(A11y::labeled(self.t("recovery.verify_button"))),
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

    fn build_confirm_voucher_screen(&self) -> ScreenModel {
        let claim = self.parsed_claim.as_ref();
        let old_prefix = claim
            .map(|c| c.old_pk_hex.chars().take(16).collect::<String>())
            .unwrap_or_else(|| "unknown".into());
        let new_prefix = claim
            .map(|c| c.new_pk_hex.chars().take(16).collect::<String>())
            .unwrap_or_else(|| "unknown".into());
        let is_expired = claim.map(|c| c.is_expired).unwrap_or(false);

        let mut components = vec![Component::InfoPanel {
            id: "claim_details".into(),
            icon: Some("lifebuoy".into()),
            title: self.t("recovery.claim_details_title"),
            items: vec![
                InfoItem {
                    icon: None,
                    title: self.t("recovery.old_id_label"),
                    detail: format!("{old_prefix}…"),
                },
                InfoItem {
                    icon: None,
                    title: self.t("recovery.new_id_label"),
                    detail: format!("{new_prefix}…"),
                },
            ],
            a11y: None,
        }];

        if is_expired {
            components.push(Component::StatusIndicator {
                id: "expired_warning".into(),
                icon: Some("warning".into()),
                title: self.t("recovery.claim_expired_title"),
                detail: Some(self.t("recovery.claim_expired_detail")),
                status: Status::Failed,
                status_label: self.t(Status::Failed.label_key()),
                a11y: None,
            });
        } else {
            components.push(Component::StatusIndicator {
                id: "verify_reminder".into(),
                icon: Some("warning".into()),
                title: self.t("recovery.verify_in_person_title"),
                detail: Some(self.t("recovery.verify_in_person_warning")),
                status: Status::Warning,
                status_label: self.t(Status::Warning.label_key()),
                a11y: None,
            });
        }

        ScreenModel {
            screen_id: "recovery_help".into(),
            title: self.t("recovery.confirm_voucher"),
            subtitle: None,
            components,
            contextual_actions: vec![
                ScreenAction {
                    id: "create_voucher".into(),
                    label: self.t("recovery.create_voucher_button"),
                    style: ActionStyle::Primary,
                    enabled: !is_expired,
                    a11y: Some(A11y::labeled(self.t("recovery.create_voucher_button"))),
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

    fn build_show_voucher_screen(&self) -> ScreenModel {
        let voucher = self
            .voucher_data
            .as_deref()
            .unwrap_or("(voucher unavailable)");

        ScreenModel {
            screen_id: "recovery_help".into(),
            title: self.t("recovery.voucher_created_title"),
            subtitle: None,
            components: vec![
                Component::StatusIndicator {
                    id: "voucher_ready".into(),
                    icon: Some("checkmark.circle.fill".into()),
                    title: self.t("recovery.voucher_created_status"),
                    detail: Some(self.t("recovery.give_voucher_detail")),
                    status: Status::Success,
                    status_label: self.t(Status::Success.label_key()),
                    a11y: None,
                },
                Component::Text {
                    id: "voucher_data".into(),
                    content: voucher.into(),
                    style: TextStyle::Caption,
                },
            ],
            contextual_actions: vec![
                ScreenAction {
                    id: "copy_voucher".into(),
                    label: self.t("recovery.copy_voucher_data"),
                    style: ActionStyle::Secondary,
                    enabled: true,
                    a11y: Some(A11y::labeled(self.t("recovery.copy_voucher_data"))),
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
}

impl WorkflowEngine for RecoveryHelpEngine {
    fn engine_output(&self) -> Option<crate::ui::EngineOutput> {
        Some(crate::ui::EngineOutput::RecoveryHelp {
            claim_input: self.claim_input().to_string(),
        })
    }

    fn apply_update(&mut self, update: crate::ui::EngineUpdate) -> bool {
        let crate::ui::EngineUpdate::RecoveryHelp(update) = update else {
            return false;
        };
        match update {
            crate::ui::RecoveryHelpUpdate::ParsedClaim(claim) => self.set_parsed_claim(claim),
            crate::ui::RecoveryHelpUpdate::ClaimParseError(message) => {
                self.set_claim_parse_error(message)
            }
            crate::ui::RecoveryHelpUpdate::VoucherData(data) => self.set_voucher_data(data),
        }
        true
    }

    fn current_screen(&self) -> ScreenModel {
        self.build_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        match (&self.step, action) {
            // Info → PasteClaim
            (HelpStep::Info, UserAction::ActionPressed { ref action_id })
                if action_id == "vouch" =>
            {
                self.step = HelpStep::PasteClaim;
                self.claim_input.clear();
                self.claim_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // PasteClaim: text input updates
            (
                HelpStep::PasteClaim,
                UserAction::TextChanged {
                    ref component_id,
                    ref value,
                },
            ) if component_id == "claim_data" => {
                self.claim_input = value.clone();
                // Clear validation error as soon as the user edits.
                self.claim_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // PasteClaim: verify pressed → outer AppEngine intercept does
            // the parse and either calls set_parsed_claim (advances state)
            // or set_claim_parse_error (stays on screen with error).
            (HelpStep::PasteClaim, UserAction::ActionPressed { ref action_id })
                if action_id == "verify_claim"
                    && self.claim_input.trim().len() >= RECOVERY_CLAIM_MIN_INPUT_LEN =>
            {
                ActionResult::Complete
            }

            // PasteClaim cancel → back to Info
            (HelpStep::PasteClaim, UserAction::ActionPressed { ref action_id })
                if action_id == "cancel" =>
            {
                self.step = HelpStep::Info;
                self.claim_input.clear();
                self.claim_error = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // ConfirmVoucher: create voucher → outer AppEngine intercept
            // does the voucher creation and calls set_voucher_data.
            (HelpStep::ConfirmVoucher, UserAction::ActionPressed { ref action_id })
                if action_id == "create_voucher" =>
            {
                ActionResult::Complete
            }
            (HelpStep::ConfirmVoucher, UserAction::ActionPressed { ref action_id })
                if action_id == "back" =>
            {
                self.step = HelpStep::PasteClaim;
                self.parsed_claim = None;
                ActionResult::UpdateScreen(self.build_screen())
            }

            // ShowVoucher: copy is informational only — frontend handles
            // the clipboard write. Done navigates back to the Info screen.
            (HelpStep::ShowVoucher, UserAction::ActionPressed { ref action_id })
                if action_id == "copy_voucher" =>
            {
                ActionResult::UpdateScreen(self.build_screen())
            }
            (HelpStep::ShowVoucher, UserAction::ActionPressed { ref action_id })
                if action_id == "done" =>
            {
                ActionResult::Complete
            }

            _ => ActionResult::UpdateScreen(self.build_screen()),
        }
    }
}

// INLINE_TEST_REQUIRED: tests assert engine state machine across step
// transitions and validate setter methods that update private state.
#[cfg(test)]
mod tests {
    use super::*;

    fn vouch(engine: &mut RecoveryHelpEngine) {
        let _ = engine.handle_action(UserAction::ActionPressed {
            action_id: "vouch".into(),
        });
    }

    fn type_claim(engine: &mut RecoveryHelpEngine, value: &str) {
        let _ = engine.handle_action(UserAction::TextChanged {
            component_id: "claim_data".into(),
            value: value.into(),
        });
    }

    fn press(engine: &mut RecoveryHelpEngine, action_id: &str) -> ActionResult {
        engine.handle_action(UserAction::ActionPressed {
            action_id: action_id.into(),
        })
    }

    // @internal
    #[test]
    fn starts_on_info_screen() {
        let engine = RecoveryHelpEngine::new();
        let screen = engine.build_screen();
        assert_eq!(screen.title, "Help Others");
        assert!(
            screen
                .contextual_actions
                .iter()
                .any(|a| a.id == "vouch" && a.enabled),
            "Info screen must offer the vouch action"
        );
    }

    // @internal
    #[test]
    fn vouch_advances_to_paste_claim() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        let screen = engine.build_screen();
        assert_eq!(screen.title, "Vouch for Recovery");
        assert!(engine.is_at_paste_claim_step());
    }

    // @internal
    #[test]
    fn verify_disabled_until_input_long_enough() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);

        // Empty input → disabled
        let screen = engine.build_screen();
        let verify = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "verify_claim")
            .unwrap();
        assert!(!verify.enabled);

        // Short input → disabled
        type_claim(&mut engine, "tooshort");
        let screen = engine.build_screen();
        let verify = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "verify_claim")
            .unwrap();
        assert!(!verify.enabled);

        // 20+ chars → enabled
        type_claim(&mut engine, "this_is_long_enough_to_pass_the_guard");
        let screen = engine.build_screen();
        let verify = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "verify_claim")
            .unwrap();
        assert!(verify.enabled);
    }

    // @internal
    #[test]
    fn verify_returns_complete_so_intercept_can_parse() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        type_claim(&mut engine, "this_is_long_enough_to_pass_the_guard");

        let result = press(&mut engine, "verify_claim");
        assert!(matches!(result, ActionResult::Complete));
        // Engine stays on PasteClaim until intercept calls set_parsed_claim
        // (success path) or set_claim_parse_error (failure path).
        assert!(engine.is_at_paste_claim_step());
    }

    // @internal
    #[test]
    fn verify_short_input_stays_on_screen() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        type_claim(&mut engine, "short");

        let result = press(&mut engine, "verify_claim");
        // Action gated by length guard → no Complete signal.
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }

    // @internal
    #[test]
    fn set_parsed_claim_advances_to_confirm() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        type_claim(&mut engine, "valid_claim_payload_long_enough");
        let _ = press(&mut engine, "verify_claim");

        engine.set_parsed_claim(ParsedClaimSummary {
            old_pk_hex: "abcd1234".repeat(8),
            new_pk_hex: "ef567890".repeat(8),
            is_expired: false,
        });
        assert!(engine.is_at_confirm_voucher_step());

        let screen = engine.build_screen();
        assert_eq!(screen.title, "Confirm Voucher");
        let create = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "create_voucher")
            .unwrap();
        assert!(
            create.enabled,
            "create voucher must be enabled for non-expired claim"
        );
    }

    // @internal
    #[test]
    fn confirm_screen_disables_create_for_expired_claim() {
        let mut engine = RecoveryHelpEngine::new();
        engine.set_parsed_claim(ParsedClaimSummary {
            old_pk_hex: "00".repeat(32),
            new_pk_hex: "11".repeat(32),
            is_expired: true,
        });

        let screen = engine.build_screen();
        let create = screen
            .contextual_actions
            .iter()
            .find(|a| a.id == "create_voucher")
            .unwrap();
        assert!(!create.enabled);
        // Expired warning must be visible to the user.
        assert!(
            screen.components.iter().any(|c| matches!(
                c,
                Component::StatusIndicator { id, .. } if id == "expired_warning"
            )),
            "expired claim must show an expired_warning indicator"
        );
    }

    // @internal
    #[test]
    fn set_claim_parse_error_keeps_user_on_paste_screen() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        type_claim(&mut engine, "broken_payload_data_here");
        let _ = press(&mut engine, "verify_claim");

        engine.set_claim_parse_error("Invalid claim format");
        assert!(engine.is_at_paste_claim_step());

        let screen = engine.build_screen();
        let input = screen.components.iter().find_map(|c| match c {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "claim_data" => validation_error.clone(),
            _ => None,
        });
        assert_eq!(input.as_deref(), Some("Invalid claim format"));
    }

    // @internal
    #[test]
    fn editing_claim_clears_validation_error() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        engine.set_claim_parse_error("Invalid claim format");

        type_claim(&mut engine, "fixing_the_input_now_with_more_chars");
        let screen = engine.build_screen();
        let error = screen.components.iter().find_map(|c| match c {
            Component::TextInput {
                id,
                validation_error,
                ..
            } if id == "claim_data" => validation_error.clone(),
            _ => None,
        });
        assert_eq!(error, None);
    }

    // @internal
    #[test]
    fn create_voucher_returns_complete_so_intercept_can_sign() {
        let mut engine = RecoveryHelpEngine::new();
        engine.set_parsed_claim(ParsedClaimSummary {
            old_pk_hex: "ab".repeat(32),
            new_pk_hex: "cd".repeat(32),
            is_expired: false,
        });

        let result = press(&mut engine, "create_voucher");
        assert!(matches!(result, ActionResult::Complete));
        // Engine stays on ConfirmVoucher until intercept calls set_voucher_data.
        assert!(engine.is_at_confirm_voucher_step());
    }

    // @internal
    #[test]
    fn set_voucher_data_advances_to_show_voucher() {
        let mut engine = RecoveryHelpEngine::new();
        engine.set_voucher_data("base64voucherpayload");

        let screen = engine.build_screen();
        assert_eq!(screen.title, "Voucher Created");
        let voucher_text = screen.components.iter().find_map(|c| match c {
            Component::Text { id, content, .. } if id == "voucher_data" => Some(content.clone()),
            _ => None,
        });
        assert_eq!(voucher_text.as_deref(), Some("base64voucherpayload"));
    }

    // @internal
    #[test]
    fn done_action_completes_engine() {
        let mut engine = RecoveryHelpEngine::new();
        engine.set_voucher_data("payload");
        let result = press(&mut engine, "done");
        assert!(matches!(result, ActionResult::Complete));
    }

    // @internal
    #[test]
    fn cancel_from_paste_returns_to_info() {
        let mut engine = RecoveryHelpEngine::new();
        vouch(&mut engine);
        type_claim(&mut engine, "some_in_progress_data");
        let _ = press(&mut engine, "cancel");

        assert_eq!(engine.build_screen().title, "Help Others");
        assert_eq!(engine.claim_input(), "");
    }

    // @internal
    #[test]
    fn back_from_confirm_returns_to_paste_and_clears_claim() {
        let mut engine = RecoveryHelpEngine::new();
        engine.set_parsed_claim(ParsedClaimSummary {
            old_pk_hex: "ab".repeat(32),
            new_pk_hex: "cd".repeat(32),
            is_expired: false,
        });
        let _ = press(&mut engine, "back");

        assert!(engine.is_at_paste_claim_step());
        assert_eq!(engine.parsed_claim(), None);
    }

    // @internal
    #[test]
    fn unrelated_actions_refresh_screen() {
        let mut engine = RecoveryHelpEngine::new();
        let result = press(&mut engine, "noop_action");
        assert!(matches!(result, ActionResult::UpdateScreen(_)));
    }
}
