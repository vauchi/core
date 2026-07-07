// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for RecoveryClaimReviewEngine — incoming recovery flows.
//!
//! Two modes:
//! - Vouching: user is a guardian, creates voucher for the recovering contact.
//! - Acceptance: user reviews a completed proof and decides to accept/reject.
//!
//! Traces to: features/contact_recovery.feature
//! - @recovery @verification @isolated: low-confidence review
//! - @recovery @verification @mutual: high-confidence review
//! - @recovery @acceptance: proof acceptance

use vauchi_app::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};
use vauchi_app::ui::*;

fn vouching_engine(confidence: Confidence) -> RecoveryClaimReviewEngine {
    RecoveryClaimReviewEngine::new(
        ReviewMode::Vouching,
        ClaimContext {
            contact_name: "Alice".into(),
            old_pk_fingerprint: "AB12 CD34 EF56".into(),
            mutual_voucher_count: match confidence {
                Confidence::High => 3,
                Confidence::Medium => 1,
                Confidence::Low => 0,
            },
            threshold: 3,
            confidence,
        },
    )
}

fn acceptance_engine(confidence: Confidence) -> RecoveryClaimReviewEngine {
    RecoveryClaimReviewEngine::new(
        ReviewMode::Acceptance,
        ClaimContext {
            contact_name: "Alice".into(),
            old_pk_fingerprint: "AB12 CD34 EF56".into(),
            mutual_voucher_count: match confidence {
                Confidence::High => 3,
                Confidence::Medium => 1,
                Confidence::Low => 0,
            },
            threshold: 3,
            confidence,
        },
    )
}

// ── Screen basics ────────────────────────────────────────────────

// @internal
#[test]
fn vouching_screen_id() {
    let engine = vouching_engine(Confidence::High);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "recovery_claim_review");
}

// @internal
#[test]
fn acceptance_screen_id() {
    let engine = acceptance_engine(Confidence::High);
    let screen = engine.current_screen();
    assert_eq!(screen.screen_id, "recovery_claim_review");
}

// @internal
#[test]
fn vouching_title_shows_contact_name() {
    let engine = vouching_engine(Confidence::High);
    let screen = engine.current_screen();
    assert!(
        screen.title.contains("Alice"),
        "title should mention contact name: {}",
        screen.title
    );
}

// ── High confidence ──────────────────────────────────────────────

// @scenario: contact_recovery :: High confidence vouching shows safe message
#[test]
fn high_confidence_vouching_shows_safe_status() {
    let engine = vouching_engine(Confidence::High);
    let screen = engine.current_screen();

    let has_success_status = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { status, .. } if *status == Status::Success
        )
    });
    assert!(
        has_success_status,
        "high confidence should show success status"
    );

    let has_vouch = screen.actions.iter().any(|a| a.id == "vouch");
    assert!(has_vouch, "should have vouch action");
}

// @scenario: contact_recovery :: High confidence acceptance shows safe message
#[test]
fn high_confidence_acceptance_shows_accept() {
    let engine = acceptance_engine(Confidence::High);
    let screen = engine.current_screen();

    let has_accept = screen.actions.iter().any(|a| a.id == "accept");
    assert!(has_accept, "should have accept action");
}

// @scenario: contact_recovery :: Vouching returns ShowVoucherQr
#[test]
fn vouch_action_transitions_to_voucher_qr() {
    let mut engine = vouching_engine(Confidence::High);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "vouch".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_qr = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::QrCode { mode, .. } if *mode == QrMode::Display));
            assert!(has_qr, "vouching should show voucher QR code");
        }
        other => panic!("Expected UpdateScreen with QR, got {other:?}"),
    }
}

// ── Medium confidence ────────────────────────────────────────────

// @scenario: contact_recovery :: Medium confidence shows warning
#[test]
fn medium_confidence_shows_warning_status() {
    let engine = vouching_engine(Confidence::Medium);
    let screen = engine.current_screen();

    let has_warning = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { status, .. } if *status == Status::Warning
        )
    });
    assert!(has_warning, "medium confidence should show warning status");
}

// @scenario: contact_recovery :: Medium confidence has remind action
#[test]
fn medium_confidence_has_remind_action() {
    let engine = vouching_engine(Confidence::Medium);
    let screen = engine.current_screen();

    let has_remind = screen.actions.iter().any(|a| a.id == "remind");
    assert!(has_remind, "medium confidence should have remind action");
}

// ── Low confidence ───────────────────────────────────────────────

// @scenario: contact_recovery :: Low confidence shows pending status
#[test]
fn low_confidence_shows_pending_status() {
    let engine = vouching_engine(Confidence::Low);
    let screen = engine.current_screen();

    let has_pending = screen.components.iter().any(|c| {
        matches!(
            c,
            Component::StatusIndicator { status, .. } if *status == Status::Pending
        )
    });
    assert!(has_pending, "low confidence should show pending status");
}

// @scenario: contact_recovery :: Low confidence has verify action
#[test]
fn low_confidence_has_verify_another_way() {
    let engine = vouching_engine(Confidence::Low);
    let screen = engine.current_screen();

    let has_verify = screen.actions.iter().any(|a| a.id == "verify_other");
    assert!(has_verify, "low confidence should have verify_other action");
}

// @scenario: contact_recovery :: Verify other shows fingerprint
#[test]
fn verify_other_shows_fingerprint() {
    let mut engine = vouching_engine(Confidence::Low);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "verify_other".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_fingerprint = screen.components.iter().any(|c| match c {
                Component::Text { content, .. } => content.contains("AB12 CD34 EF56"),
                _ => false,
            });
            assert!(
                has_fingerprint,
                "should show old_pk fingerprint for verification"
            );
        }
        other => panic!("Expected UpdateScreen with fingerprint, got {other:?}"),
    }
}

// @scenario: contact_recovery :: Low confidence accept anyway shows confirm
#[test]
fn low_confidence_accept_anyway_shows_confirm() {
    let mut engine = acceptance_engine(Confidence::Low);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "accept_anyway".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            let has_confirm = screen
                .components
                .iter()
                .any(|c| matches!(c, Component::InlineConfirm { .. }));
            assert!(
                has_confirm,
                "should show inline confirm for risky acceptance"
            );
        }
        other => panic!("Expected UpdateScreen with confirm, got {other:?}"),
    }
}

// ── Reject ───────────────────────────────────────────────────────

// @scenario: contact_recovery :: Reject returns Complete
#[test]
fn reject_returns_complete() {
    let mut engine = acceptance_engine(Confidence::Low);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "reject".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "reject should return Complete"
    );
}

// ── Remind ───────────────────────────────────────────────────────

// @scenario: contact_recovery :: Remind returns Complete
#[test]
fn remind_returns_complete() {
    let mut engine = vouching_engine(Confidence::Medium);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "remind".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "remind should return Complete"
    );
}

// ── Done from voucher QR ─────────────────────────────────────────

// @scenario: contact_recovery :: Done from voucher QR returns Complete
#[test]
fn done_from_voucher_qr_returns_complete() {
    let mut engine = vouching_engine(Confidence::High);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "vouch".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "done".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "done after vouching should return Complete"
    );
}

// ── Acceptance confirm ───────────────────────────────────────────

// @scenario: contact_recovery :: Accept high confidence returns Complete
#[test]
fn accept_high_confidence_returns_complete() {
    let mut engine = acceptance_engine(Confidence::High);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "accept".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "accept should return Complete"
    );
}

// @scenario: contact_recovery :: Confirm risky accept returns Complete
#[test]
fn confirm_risky_accept_returns_complete() {
    let mut engine = acceptance_engine(Confidence::Low);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "accept_anyway".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "confirm_accept".into(),
    });
    assert!(
        matches!(result, ActionResult::Complete),
        "confirm_accept should return Complete"
    );
}

// @internal
#[test]
fn cancel_from_confirm_returns_to_review() {
    let mut engine = acceptance_engine(Confidence::Low);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "accept_anyway".into(),
    });
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "cancel".into(),
    });
    match result {
        ActionResult::UpdateScreen(screen) => {
            // Should be back at the review screen (no InlineConfirm)
            let has_reject = screen.actions.iter().any(|a| a.id == "reject");
            assert!(has_reject, "should be back at review with reject action");
        }
        other => panic!("Expected UpdateScreen, got {other:?}"),
    }
}

// @internal
#[test]
fn unknown_action_returns_update_screen() {
    let mut engine = vouching_engine(Confidence::High);
    let result = engine.handle_action(UserAction::ActionPressed {
        action_id: "unknown".into(),
    });
    assert!(
        matches!(result, ActionResult::UpdateScreen(_)),
        "unknown action should return UpdateScreen"
    );
}

// @internal
#[test]
fn was_cancelled_after_reject() {
    let mut engine = acceptance_engine(Confidence::Low);
    let _ = engine.handle_action(UserAction::ActionPressed {
        action_id: "reject".into(),
    });
    assert!(engine.was_cancelled(), "reject should mark as cancelled");
}
