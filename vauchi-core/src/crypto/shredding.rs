// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crypto-Shredding Key Hierarchy
//!
//! Implements the Shredding Master Key (SMK) and derived keys for
//! cryptographic erasure of all persisted data.
//!
//! Key hierarchy:
//! ```text
//! Master Seed (256-bit)
//!     └── SMK = HKDF(ikm=master_seed, info="Vauchi_Shred_Key_v2")
//!         ├── SEK = HKDF(ikm=SMK, info="Vauchi_Storage_Key_v2")
//!         │   └── encrypts all local SQLite data
//!         └── FKEK = HKDF(ikm=SMK, info="Vauchi_FileKey_Key_v2")
//!             └── encrypts FileKeyStorage encryption_key
//! ```
//!
//! Destroying the SMK renders all locally persisted data irrecoverable.
//!
//! Design principle (DP-1): SMK is derived once from master_seed at identity
//! creation or migration, then stored in SecureStorage. At boot, SMK is loaded
//! from SecureStorage — never re-derived — to avoid bootstrap deadlock.

use crate::crypto::{SymmetricKey, HKDF};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// HKDF info string for SMK derivation from master_seed.
/// v2: Fixed HKDF parameter ordering (seed as IKM, not salt).
const SMK_INFO: &[u8] = b"Vauchi_Shred_Key_v2";
/// HKDF info string for SEK derivation from SMK.
const SEK_INFO: &[u8] = b"Vauchi_Storage_Key_v2";
/// HKDF info string for FKEK derivation from SMK.
const FKEK_INFO: &[u8] = b"Vauchi_FileKey_Key_v2";

/// Shredding Master Key — the root of the crypto-shredding key hierarchy.
///
/// Destroying this key renders all data encrypted under SEK or FKEK
/// computationally irrecoverable.
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct ShreddingMasterKey {
    bytes: [u8; 32],
}

impl std::fmt::Debug for ShreddingMasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShreddingMasterKey")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

impl ShreddingMasterKey {
    /// Derives SMK from master_seed using HKDF.
    ///
    /// Called once at identity creation or during migration from pre-SMK storage.
    /// The resulting SMK should be persisted to SecureStorage immediately.
    pub fn derive_from_seed(master_seed: &[u8; 32]) -> Self {
        let bytes = HKDF::derive_key(None, master_seed, SMK_INFO);
        Self { bytes }
    }

    /// Creates an SMK from raw bytes loaded from SecureStorage at boot.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self { bytes }
    }

    /// Returns the raw bytes for persistence to SecureStorage.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Derives the Storage Encryption Key (SEK) from this SMK.
    ///
    /// SEK encrypts all SQLite columns containing sensitive data.
    /// Called at boot after loading SMK from SecureStorage.
    pub fn derive_sek(&self) -> SymmetricKey {
        let bytes = HKDF::derive_key(None, &self.bytes, SEK_INFO);
        SymmetricKey::from_bytes(bytes)
    }

    /// Derives the File Key Encryption Key (FKEK) from this SMK.
    ///
    /// FKEK encrypts FileKeyStorage's encryption_key.
    /// Called at boot after loading SMK from SecureStorage.
    pub fn derive_fkek(&self) -> SymmetricKey {
        let bytes = HKDF::derive_key(None, &self.bytes, FKEK_INFO);
        SymmetricKey::from_bytes(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_smk_derivation_deterministic() {
        let seed = [0x42u8; 32];
        let smk1 = ShreddingMasterKey::derive_from_seed(&seed);
        let smk2 = ShreddingMasterKey::derive_from_seed(&seed);
        assert_eq!(smk1.as_bytes(), smk2.as_bytes());
    }

    #[test]
    fn test_sek_derivation_deterministic() {
        let seed = [0x42u8; 32];
        let smk = ShreddingMasterKey::derive_from_seed(&seed);
        let sek1 = smk.derive_sek();
        let sek2 = smk.derive_sek();
        assert_eq!(sek1.as_bytes(), sek2.as_bytes());
    }

    #[test]
    fn test_fkek_derivation_deterministic() {
        let seed = [0x42u8; 32];
        let smk = ShreddingMasterKey::derive_from_seed(&seed);
        let fkek1 = smk.derive_fkek();
        let fkek2 = smk.derive_fkek();
        assert_eq!(fkek1.as_bytes(), fkek2.as_bytes());
    }

    #[test]
    fn test_different_seeds_produce_different_smks() {
        let seed1 = [0x01u8; 32];
        let seed2 = [0x02u8; 32];
        let smk1 = ShreddingMasterKey::derive_from_seed(&seed1);
        let smk2 = ShreddingMasterKey::derive_from_seed(&seed2);
        assert_ne!(smk1.as_bytes(), smk2.as_bytes());
    }

    #[test]
    fn test_smk_sek_fkek_all_distinct() {
        let seed = [0x42u8; 32];
        let smk = ShreddingMasterKey::derive_from_seed(&seed);
        let sek = smk.derive_sek();
        let fkek = smk.derive_fkek();

        assert_ne!(smk.as_bytes(), sek.as_bytes());
        assert_ne!(smk.as_bytes(), fkek.as_bytes());
        assert_ne!(sek.as_bytes(), fkek.as_bytes());
    }

    #[test]
    fn test_from_bytes_round_trip() {
        let seed = [0x42u8; 32];
        let smk_original = ShreddingMasterKey::derive_from_seed(&seed);
        let bytes = *smk_original.as_bytes();
        let smk_restored = ShreddingMasterKey::from_bytes(bytes);

        assert_eq!(smk_original.as_bytes(), smk_restored.as_bytes());

        // Derived keys should also match
        let sek_original = smk_original.derive_sek();
        let sek_restored = smk_restored.derive_sek();
        assert_eq!(sek_original.as_bytes(), sek_restored.as_bytes());
    }

    #[test]
    fn test_debug_redacts_key_material() {
        let seed = [0x42u8; 32];
        let smk = ShreddingMasterKey::derive_from_seed(&seed);
        let debug_str = format!("{:?}", smk);
        assert!(debug_str.contains("REDACTED"));
        assert!(!debug_str.contains("42"));
    }

    #[test]
    fn test_smk_is_not_identity_or_exchange_key() {
        // SMK must be distinct from existing key derivations
        let seed = [0x42u8; 32];
        let smk = ShreddingMasterKey::derive_from_seed(&seed);

        // Identity key uses SigningKeyPair::from_seed (raw seed, no HKDF)
        // Exchange key uses HKDF with seed as IKM
        let exchange_key = HKDF::derive_key(None, &seed, b"Vauchi_Exchange_Seed_v2");

        assert_ne!(
            smk.as_bytes(),
            &seed,
            "SMK must differ from raw master_seed"
        );
        assert_ne!(
            smk.as_bytes(),
            &exchange_key,
            "SMK must differ from exchange key"
        );
    }
}
