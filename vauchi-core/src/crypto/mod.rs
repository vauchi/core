// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

pub mod cek;
pub mod chain;
pub mod encryption;
pub mod hkdf;
pub mod ohttp_key_trust;
pub mod padding;
#[cfg(kani)]
mod padding_proofs;
pub mod password_kdf;
pub mod pinning;
pub mod random;
pub mod ratchet;
#[cfg(feature = "testing")]
pub mod shamir;
#[cfg(not(feature = "testing"))]
pub(crate) mod shamir;
pub mod shredding;
pub mod signing;
pub mod x25519;

pub use chain::{ChainError, ChainKey, MessageKey};
pub use encryption::{SymmetricKey, decrypt, decrypt_with_ad, encrypt, encrypt_with_ad};
pub use hkdf::{HKDF, KDFError};
pub use password_kdf::{PasswordKdfError, derive_key_argon2id};
pub use random::{random_bytes, random_fill};
pub use ratchet::{DoubleRatchetState, RATCHET_STATE_VERSION, RatchetError, RatchetMessage};
pub use shredding::ShreddingMasterKey;
pub use signing::{PublicKey, Signature, SigningKeyPair};
pub use x25519::{DhError, X25519KeyPair};
