// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod backend;
pub mod cek;
pub mod chain;
pub mod encryption;
pub mod kdf;
pub mod padding;
pub mod password_kdf;
pub mod ratchet;
pub mod shredding;
pub mod signing;

pub use chain::{ChainError, ChainKey, MessageKey};
pub use encryption::{decrypt, decrypt_with_ad, encrypt, encrypt_with_ad, SymmetricKey};
pub use kdf::{KDFError, HKDF};
pub use password_kdf::{derive_key_argon2id, PasswordKdfError};
pub use ratchet::{DoubleRatchetState, RatchetError, RatchetMessage, RATCHET_STATE_VERSION};
pub use shredding::ShreddingMasterKey;
pub use signing::{PublicKey, Signature, SigningKeyPair};
