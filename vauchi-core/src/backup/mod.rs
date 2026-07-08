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
pub mod key_shard;

pub use contact_backup::{BackupError, export_contact_backup, import_contact_backup};
pub use full_backup::{
    BackupSections, FullBackupEnvelope, FullBackupIdentityData, IdentitySection, LabelSection,
    export_full_backup, export_guardian_backup, extract_master_seed, import_full_backup,
    import_guardian_backup, restore_contacts_from_envelope,
};
pub use key_shard::{
    BackupKey, BackupKeyShard, KeyShardConfig, KeyShardError, open_share_for_guardian,
    reconstruct_backup_key, seal_share_for_guardian, split_backup_key,
};
