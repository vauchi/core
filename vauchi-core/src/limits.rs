// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared payload bounds used by backup and presentation.

/// Maximum accepted password-based full backup size (32 MiB). A backup
/// arrives through a file pick, so this must not exceed the shell
/// protocol's `MAX_FILE_PICK_BYTES`; `tests/it/payload_bounds_tests.rs` pins
/// the two equal, because `limits` sits below `platform` in the layer graph
/// (VRS06) and may not import it.
pub const MAX_FULL_BACKUP_BYTES: usize = 32 * 1024 * 1024;
