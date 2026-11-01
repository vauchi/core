// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The shell protocol's file-pick ceiling and Core's full-backup limit are
//! defined apart (the protocol leaves Core for the contract package,
//! ADR-078, and `limits` may not import `platform`, VRS06) and must stay
//! equal: a full backup arrives through one file pick (vauchi/private#1013).

use vauchi_core::{MAX_FILE_PICK_BYTES, MAX_FULL_BACKUP_BYTES};

// @internal
#[test]
fn a_full_backup_fits_exactly_one_file_pick() {
    assert_eq!(MAX_FULL_BACKUP_BYTES, MAX_FILE_PICK_BYTES);
    assert_eq!(
        MAX_FILE_PICK_BYTES,
        32 * 1024 * 1024,
        "the ceiling is 32 MiB"
    );
}
