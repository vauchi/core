// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Strongly-typed identifier newtypes for swap-argument safety.
//!
//! Wraps raw `[u8; 32]` values that share a shape but mean different
//! things (e.g. `old_pk`, `new_pk`, `voucher_pk` on the recovery
//! structs). Accessors that previously returned `&[u8; 32]` now
//! return `&IdentityKey`, so cross-identifier swaps at call
//! boundaries that mix newtype kinds become compile errors as more
//! kinds are introduced (Phase 1B / 2 / 3 of the
//! `2026-05-21-wire-identifier-newtypes` problem record).
//!
//! Wire-shape stability is preserved via `#[serde(transparent)]` so
//! the on-disk JSON shape of `RecoveryProgress` (which contains a
//! `RecoveryClaim` and `Vec<RecoveryVoucher>` and is persisted via
//! `serde_json::to_vec`) does not change across the newtype
//! migration.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A 32-byte Ed25519 public key, opaque at the type level.
///
/// Used by the recovery structs (`RecoveryClaim`, `RecoveryVoucher`,
/// `RecoveryProof`) to tag `old_pk` / `new_pk` / `voucher_pk` with
/// a single nominal type that prevents accidental cross-kind swaps
/// once more identifier newtypes are introduced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct IdentityKey([u8; 32]);

impl IdentityKey {
    /// Wraps raw bytes into an `IdentityKey`.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying 32 bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Consumes the `IdentityKey`, returning the underlying bytes.
    pub const fn into_bytes(self) -> [u8; 32] {
        self.0
    }
}

impl From<[u8; 32]> for IdentityKey {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl From<&[u8; 32]> for IdentityKey {
    fn from(bytes: &[u8; 32]) -> Self {
        Self(*bytes)
    }
}

impl AsRef<[u8]> for IdentityKey {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; 32]> for IdentityKey {
    fn as_ref(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for IdentityKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
