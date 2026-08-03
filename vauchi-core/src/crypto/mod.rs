// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use rand_core::RngCore;

pub mod cek;
pub mod chain;
pub mod encryption;
pub mod kdf;
pub mod padding;
pub mod password_kdf;
pub mod ratchet;
#[cfg(feature = "testing")]
pub mod shamir;
#[cfg(not(feature = "testing"))]
pub(crate) mod shamir;
pub mod shredding;
pub mod signing;
pub mod x3dh;

/// Generate cryptographically secure random bytes.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand_core::OsRng.fill_bytes(&mut buf);
    buf
}

/// Fill a mutable slice with cryptographically secure random bytes.
pub fn random_fill(buf: &mut [u8]) {
    rand_core::OsRng.fill_bytes(buf);
}

pub use chain::{ChainError, ChainKey, MessageKey};
pub use encryption::{SymmetricKey, decrypt, decrypt_with_ad, encrypt, encrypt_with_ad};
pub use kdf::{HKDF, KDFError};
pub use password_kdf::{PasswordKdfError, derive_key_argon2id};
pub use ratchet::{DoubleRatchetState, RATCHET_STATE_VERSION, RatchetError, RatchetMessage};
pub use shredding::ShreddingMasterKey;
pub use signing::{PublicKey, Signature, SigningKeyPair};
pub use x3dh::{DhError, X3DHKeyPair};
