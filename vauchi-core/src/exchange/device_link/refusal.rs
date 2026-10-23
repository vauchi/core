// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The host's word to the joiner that it ended the ceremony, and why
//! (vauchi/private#780).
//!
//! It travels on the joiner's return channel in place of a response, under
//! its own key derived from the link key, so it can never decrypt as a
//! request or a response (ADR-007).

use crate::crypto::{HKDF, SymmetricKey, decrypt, encrypt};

use super::super::ExchangeError;

const REFUSAL_KEY_INFO: &[u8] = b"vauchi-device-link-refusal-v1";
const REFUSAL_FORMAT_VERSION: u8 = 1;

/// Why the host ended the ceremony.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceLinkRefusalReason {
    Declined,
    ConfirmationTimedOut,
    DeviceNameTaken,
}

impl DeviceLinkRefusalReason {
    fn code(self) -> u8 {
        match self {
            Self::Declined => 1,
            Self::ConfirmationTimedOut => 2,
            Self::DeviceNameTaken => 3,
        }
    }

    fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Declined),
            2 => Some(Self::ConfirmationTimedOut),
            3 => Some(Self::DeviceNameTaken),
            _ => None,
        }
    }
}

fn refusal_key(link_key: &[u8; 32]) -> SymmetricKey {
    SymmetricKey::from_bytes(*HKDF::derive_key(None, link_key, REFUSAL_KEY_INFO))
}

pub(super) fn encrypt_refusal(
    link_key: &[u8; 32],
    reason: DeviceLinkRefusalReason,
) -> Result<Vec<u8>, ExchangeError> {
    encrypt(
        &refusal_key(link_key),
        &[REFUSAL_FORMAT_VERSION, reason.code()],
    )
    .map_err(|_| ExchangeError::CryptoError)
}

pub(super) fn decrypt_refusal(
    link_key: &[u8; 32],
    ciphertext: &[u8],
) -> Result<DeviceLinkRefusalReason, ExchangeError> {
    let plaintext =
        decrypt(&refusal_key(link_key), ciphertext).map_err(|_| ExchangeError::CryptoError)?;
    match plaintext.as_slice() {
        [REFUSAL_FORMAT_VERSION, code] => {
            DeviceLinkRefusalReason::from_code(*code).ok_or(ExchangeError::InvalidQRFormat)
        }
        _ => Err(ExchangeError::InvalidQRFormat),
    }
}
