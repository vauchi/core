// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The relay purge an identity deletion sends (#711).

use crate::api::error::{VauchiError, VauchiResult};
use crate::api::pre_signed::purge_signing_message;

use super::Vauchi;

impl Vauchi {
    /// Asks the relay to erase what it holds for this identity, and with
    /// `erase_guardian_set` its guardian set too.
    ///
    /// Signed now rather than pre-signed: the relay accepts a purge only
    /// within 60 seconds of its timestamp. Returns the blob count the relay
    /// reports, if any.
    pub fn purge_relay_data(&self, erase_guardian_set: bool) -> VauchiResult<Option<usize>> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;
        let public_key = *identity.signing_public_key();
        let purge_token: [u8; 32] = crate::crypto::random_bytes();
        let timestamp = self.clock.unix_seconds();
        let message =
            purge_signing_message(&public_key, &purge_token, timestamp, erase_guardian_set);
        let signature = identity.sign(&message);
        let public_key_hex = hex::encode(public_key);
        Ok(self.create_guardian_transport().purge_scoped(
            &public_key_hex,
            &public_key_hex,
            &hex::encode(purge_token),
            &hex::encode(signature.as_bytes()),
            timestamp,
            erase_guardian_set,
        )?)
    }
}
