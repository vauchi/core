// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Reachability test for `BackupRecoveryEngine`.
//!
//! Multi-step state machine — `ChooseMode → EnterPassword →
//! {ConfirmReplace | ConfirmPassword} → Processing → {Complete |
//! Failed}` — with a **distinct `screen_id` per step**, so BFS
//! traverses across screens (unlike `recovery.rs`, where every
//! `RecoveryStep` shares one `recovery_status` id and dedup
//! collapses them).
//!
//! ## What BFS reaches
//!
//! The structural walker fills text inputs as it explores, so it
//! clears the `EnterPassword` "continue" gate
//! (`backup_recovery.rs:493`) and advances past it. From the
//! `has_identity = true` restore path it reaches `ConfirmReplace`
//! (`backup_confirm_replace`); from the create path it reaches
//! `ConfirmPassword` (`backup_confirm`). The reachable affordance
//! set is therefore `{create, restore}` (choose) ∪
//! `{back, continue}` (password / confirm) ∪
//! `{confirm_replace, cancel_replace}` (replace).
//!
//! ## What stays unreachable (pinned elsewhere)
//!
//! `Processing` (`backup_processing`) renders **no** affordances,
//! and the only exits from it — `Complete` (`done`) and `Failed`
//! (`retry`/`cancel`) — are entered solely via the
//! `processing_complete` / `processing_failed` hardware callbacks,
//! which a structural UI walk cannot fire. Those three ids are
//! exercised end-to-end by
//! `core/vauchi-app/tests/it/backup_recovery_confirm_replace_tests.rs`
//! and `core/vauchi-core/tests/it/backup_recovery_engine_tests.rs`.
//! Declaring them here would make them orphan handlers (declared,
//! no affordance emitted on a reachable screen), so they are
//! deliberately excluded — same discipline as `recovery.rs`.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{BackupRecoveryEngine, WorkflowEngine};

/// Action ids emitted by the BFS-reachable screens (`backup_choose`,
/// `backup_password`, `backup_confirm`, `backup_confirm_replace`)
/// and consumed by `BackupRecoveryEngine::handle_action` —
/// `core/vauchi-app/src/ui/backup_recovery.rs`.
const HANDLED: &[&str] = &[
    "create",
    "restore",
    "back",
    "continue",
    "confirm_replace",
    "cancel_replace",
];

fn factory() -> BackupRecoveryEngine {
    // Mode unset → starts on `ChooseMode` (`backup_choose`).
    // `has_identity = true` is the more demanding path: a restore
    // routes through `ConfirmReplace` downstream rather than
    // straight to `Processing`. The reachability surface is
    // identical either way (both gated behind the password), but
    // pinning the identity-present construction documents the
    // realistic post-onboarding entry state.
    BackupRecoveryEngine::new(None, true)
}

// @internal
#[test]
fn backup_choose_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");
    assert_reachability_across_screens(factory, HANDLED);
}
