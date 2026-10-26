// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reachability test for `BackupRecoveryEngine`.
//!
//! Multi-step state machine — `ChooseMode → {EnterPassword →
//! [ConfirmPassword] → Processing → {Complete | Failed} |
//! RestoreNeedsEmptyApp}` — with a **distinct `screen_id` per step**, so BFS
//! traverses across screens (unlike `recovery.rs`, where every
//! `RecoveryStep` shares one `recovery_status` id and dedup
//! collapses them).
//!
//! ## What BFS reaches
//!
//! The structural walker fills text inputs as it explores, so it
//! clears the `EnterPassword` "continue" gate
//! and advances past it; from the create path it reaches
//! `ConfirmPassword` (`backup_confirm`). Export and Restore on the
//! choose screen are rows (#459), `ListItemSelected` pass-throughs the
//! walk follows. On an empty app the declared `ActionPressed` set is
//! `{back, continue, choose_file}`; on a device with an identity a
//! restore stops at `RestoreNeedsEmptyApp` (`backup_restore_needs_empty`,
//! vauchi/private#819), so `choose_file` is not reachable there.
//!
//! ## What stays unreachable (pinned elsewhere)
//!
//! `Processing` (`backup_processing`) renders **no** affordances,
//! and the only exits from it — `Complete` (`done`) and `Failed`
//! (`retry`/`cancel`) — are entered solely via the
//! `processing_complete` / `processing_failed` hardware callbacks,
//! which a structural UI walk cannot fire. Those three ids are
//! exercised end-to-end by
//! `core/vauchi-app/tests/it/backup_restore_needs_empty_app_tests.rs`
//! and `core/vauchi-core/tests/it/backup_recovery_engine_tests.rs`.
//! Declaring them here would make them orphan handlers (declared,
//! no affordance emitted on a reachable screen), so they are
//! deliberately excluded — same discipline as `recovery.rs`.

use vauchi_app::ui::testing::assert_reachability_across_screens;
use vauchi_app::ui::{BackupRecoveryEngine, WorkflowEngine};

/// Action ids emitted by the BFS-reachable screens of an empty app
/// (`backup_choose`, `backup_password`, `backup_confirm`) and consumed
/// by `BackupRecoveryEngine::handle_action` —
/// `core/vauchi-app/src/ui/backup_recovery.rs`.
const HANDLED: &[&str] = &[
    "back",
    "continue",
    // Restore-only: asks the shell for the backup file. A real export
    // cannot arrive through the paste field (2.2 MB against a 4 KB
    // MAX_EVENT_INPUT_VALUE_BYTES), so this is the load-bearing path
    // (2026-09-08-full-backup-restore-cannot-be-pasted).
    "choose_file",
];

/// On a device with an identity a restore ends at the explanation, whose
/// only action is `back`.
const HANDLED_WITH_IDENTITY: &[&str] = &["back", "continue"];

fn factory() -> BackupRecoveryEngine {
    BackupRecoveryEngine::new(None, false, vauchi_app::i18n::Locale::ENGLISH)
}

fn factory_with_identity() -> BackupRecoveryEngine {
    BackupRecoveryEngine::new(None, true, vauchi_app::i18n::Locale::ENGLISH)
}

// @internal
#[test]
fn backup_choose_screen_is_reachable() {
    let engine = factory();
    assert_eq!(engine.current_screen().screen_id, "backup_choose");
    assert_reachability_across_screens(factory, HANDLED);
}

// @internal
#[test]
fn backup_screens_with_an_identity_are_reachable() {
    assert_reachability_across_screens(factory_with_identity, HANDLED_WITH_IDENTITY);
}
