// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Backup module
//!
//! Provides encrypted backup and restore functionality:
//! - **v1** (contact backup): contacts only, `export_contact_backup` / `import_contact_backup`
//! - **v2** (identity backup): master seed + device info, lives in `identity/backup.rs`
//! - **v3** (full backup): identity + contacts + own card + labels in one envelope

pub mod contact_backup;
pub mod full_backup;
#[cfg(feature = "network-rustls")]
mod guardian_recovery;
pub mod key_shard;

#[cfg(feature = "network-rustls")]
pub(crate) use guardian_recovery::recover_from_shards;

pub use contact_backup::{BackupError, export_contact_backup, import_contact_backup};
#[cfg(feature = "network-rustls")]
pub(crate) use full_backup::guardian_backup_key_matches;
pub use full_backup::{
    BackupSections, FullBackupEnvelope, FullBackupIdentityData, IdentitySection, LabelSection,
    export_full_backup, export_guardian_backup, extract_master_seed, import_full_backup,
    import_guardian_backup, restore_contacts_from_envelope,
};
#[cfg(feature = "network-rustls")]
pub(crate) use full_backup::{decode_guardian_backup_hex, guardian_backup_metadata};
pub use key_shard::{
    BackupKey, BackupKeyShard, GuardianBackupMetadata, KeyShardConfig, KeyShardError,
    open_share_for_guardian, reconstruct_backup_key, seal_share_for_guardian, split_backup_key,
};
