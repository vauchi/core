// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A contact's recovery state: accepting its recovery to a new key, and
//! whether it is trusted as one of this identity's guardians. Split out of
//! `contact/mod.rs` to keep it under the 1000-line limit.

use super::*;

impl Contact {
    /// Takes the shared key of an in-person exchange for a contact restored
    /// from a guardian backup, which has no relationship on this device yet
    /// (vauchi/private#796, D20). The old shared key, which a thief holding
    /// the old device may also have, is discarded.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn adopt_exchange_shared_key(
        &mut self,
        shared_key: SymmetricKey,
        now: u64,
    ) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.shared_key = shared_key;
        data.exchange_timestamp = now;
        Ok(())
    }

    /// Accepts a recovery, updating the contact's public key and shared secret.
    ///
    /// This is called when the user accepts a recovery proof from this contact.
    /// The old shared secret is discarded and fingerprint verification is reset.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn accept_recovery(
        &mut self,
        new_public_key: [u8; 32],
        new_shared_key: SymmetricKey,
        now: u64,
    ) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.public_key = new_public_key;
        data.shared_key = new_shared_key;
        data.fingerprint_verified = false;
        data.has_recovered = true;
        data.exchange_timestamp = now;
        self.id = hex::encode(new_public_key);
        Ok(())
    }

    /// Accepts a recovery with a new contact card.
    ///
    /// This is called when the recovering contact also provides an updated card.
    pub fn accept_recovery_with_card(
        &mut self,
        new_public_key: [u8; 32],
        new_shared_key: SymmetricKey,
        new_card: ContactCard,
        now: u64,
    ) -> Result<(), ContactError> {
        self.accept_recovery(new_public_key, new_shared_key, now)?;
        self.update_card(new_card, now);
        Ok(())
    }

    /// Returns whether this contact is trusted for recovery purposes.
    /// Returns `false` for imported contacts.
    pub fn is_recovery_trusted(&self) -> bool {
        self.kind
            .exchanged_data()
            .is_some_and(|d| d.recovery_trusted)
    }

    /// Marks this contact as trusted for recovery.
    ///
    /// Returns `Err` if called on an imported contact, a blocked contact,
    /// or if trust level is below High (not in-person verified).
    pub fn trust_for_recovery(&mut self) -> Result<(), ContactError> {
        let _data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;

        if self.is_blocked() {
            return Err(ContactError::ContactIsBlocked);
        }

        let level = self.trust_level();
        if !matches!(level, TrustLevel::High | TrustLevel::Verified) {
            return Err(ContactError::InsufficientTrustLevel(level));
        }

        // Re-borrow after trust_level() released the immutable ref
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = true;
        Ok(())
    }

    /// Removes recovery trust from this contact.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn untrust_for_recovery(&mut self) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = false;
        Ok(())
    }

    /// Sets the recovery trust status directly.
    ///
    /// Returns `Err` if called on an imported contact.
    pub fn set_recovery_trusted(&mut self, trusted: bool) -> Result<(), ContactError> {
        let data = self
            .kind
            .exchanged_data_mut()
            .ok_or(ContactError::OperationRequiresExchangedContact)?;
        data.recovery_trusted = trusted;
        Ok(())
    }
}
