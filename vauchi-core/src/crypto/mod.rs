// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

pub mod chain;
pub mod encryption;
pub mod kdf;
pub mod ratchet;
pub mod signing;

pub use chain::{ChainError, ChainKey, MessageKey};
pub use encryption::{decrypt, encrypt, SymmetricKey};
pub use kdf::{KDFError, HKDF};
pub use ratchet::{DoubleRatchetState, RatchetError, RatchetMessage};
pub use signing::{PublicKey, Signature, SigningKeyPair};
