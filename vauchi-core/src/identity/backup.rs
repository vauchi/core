// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Identity Backup Module
//!
//! Handles encrypted backup and restore of identity data.

/// Encrypted identity backup.
pub struct IdentityBackup {
    data: Vec<u8>,
}

impl IdentityBackup {
    /// Creates a new backup from encrypted data.
    pub fn new(data: Vec<u8>) -> Self {
        IdentityBackup { data }
    }

    /// Returns the encrypted backup data.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    /// Returns mutable access to the backup data (for testing).
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}
