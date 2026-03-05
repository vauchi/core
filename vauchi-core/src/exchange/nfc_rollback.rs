// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! NFC Exchange Rollback
//!
//! Trait for rolling back contact storage and ratchet state on exchange failure.
//! Enforces the atomicity contract: neither side commits until both have
//! decrypted and validated.

use super::ExchangeError;

/// Rollback operations for NFC exchange atomicity.
///
/// Storage adapters implement this trait. The default `NoopNfcRollback`
/// is used in tests.
pub trait NfcRollback: Send + Sync {
    /// Deletes a contact record that was saved during a failed exchange.
    fn rollback_contact(&self, contact_id: &str) -> Result<(), ExchangeError>;

    /// Wipes ratchet state initialized during a failed exchange.
    fn rollback_ratchet(&self, contact_id: &str) -> Result<(), ExchangeError>;

    /// Performs full rollback: contact + ratchet.
    fn rollback_all(&self, contact_id: &str) -> Result<(), ExchangeError> {
        self.rollback_contact(contact_id)?;
        self.rollback_ratchet(contact_id)?;
        Ok(())
    }
}

/// No-op rollback for testing.
pub struct NoopNfcRollback;

impl NfcRollback for NoopNfcRollback {
    fn rollback_contact(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        Ok(())
    }

    fn rollback_ratchet(&self, _contact_id: &str) -> Result<(), ExchangeError> {
        Ok(())
    }
}
