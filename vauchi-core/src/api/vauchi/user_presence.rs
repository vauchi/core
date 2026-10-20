// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The secret a shell seals behind the operating system's own
//! user-presence check (#712 D2): Core keeps only its SHA-256, enrolls it
//! once per device, and confirms a revealed secret in constant time. A
//! "success" without the OS check carries no secret, so it confirms
//! nothing.

use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::super::error::VauchiResult;
use super::Vauchi;

/// The shell makes 32 bytes; anything outside this range is not a secret.
const SECRET_LEN: std::ops::RangeInclusive<usize> = 16..=64;

impl Vauchi {
    /// Whether this device has enrolled a user-presence secret.
    pub fn has_user_presence_secret(&self) -> VauchiResult<bool> {
        Ok(self.storage.ux().load_user_presence_digest()?.is_some())
    }

    /// Enrolls `secret` as this device's user-presence secret. `false` when
    /// one is already enrolled or `secret` has the wrong size: whoever can
    /// send events must not be able to replace the keystore's secret.
    pub fn enroll_user_presence_secret(&self, secret: &[u8]) -> VauchiResult<bool> {
        if !SECRET_LEN.contains(&secret.len()) || self.has_user_presence_secret()? {
            return Ok(false);
        }
        let digest: [u8; 32] = Sha256::digest(secret).into();
        self.storage.ux().save_user_presence_digest(&digest)?;
        Ok(true)
    }

    /// Whether `secret` is the one this device enrolled.
    pub fn confirm_user_presence_secret(&self, secret: &[u8]) -> VauchiResult<bool> {
        if !SECRET_LEN.contains(&secret.len()) {
            return Ok(false);
        }
        let Some(enrolled) = self.storage.ux().load_user_presence_digest()? else {
            return Ok(false);
        };
        let digest: [u8; 32] = Sha256::digest(secret).into();
        Ok(bool::from(digest.ct_eq(&enrolled)))
    }
}
