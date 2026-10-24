// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared payload bounds used by backup and presentation.

/// Maximum accepted password-based full backup size (32 MiB).
pub const MAX_FULL_BACKUP_BYTES: usize = 32 * 1024 * 1024;
