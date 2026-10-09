// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Keyed lookup hashes for equality queries on encrypted columns (#128).

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::crypto::{HKDF, SymmetricKey};

/// HKDF domain for the lookup hash of an imported contact's vCard UID
/// (vauchi/private#579).
pub(crate) const CONTACT_UID_LOOKUP_DOMAIN: &[u8] = b"Vauchi_Contact_Uid_HMAC_v1";

/// HMAC-SHA256 of `value` under a key derived from the storage key for
/// `domain`, so equal values match without storing them in plaintext.
pub(crate) fn lookup_hmac(key: &SymmetricKey, domain: &[u8], value: &[u8]) -> Vec<u8> {
    let hmac_key = HKDF::derive_key(None, key.as_bytes(), domain);
    let mut mac = Hmac::<Sha256>::new_from_slice(&*hmac_key).expect("HMAC accepts any key length");
    mac.update(value);
    mac.finalize().into_bytes().to_vec()
}
