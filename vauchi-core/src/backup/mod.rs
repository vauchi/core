// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup module
//!
//! Provides encrypted backup and restore functionality for contacts.
//! Identity backup lives in `identity/backup.rs`.

pub mod contact_backup;

pub use contact_backup::{BackupError, export_contact_backup, import_contact_backup};
