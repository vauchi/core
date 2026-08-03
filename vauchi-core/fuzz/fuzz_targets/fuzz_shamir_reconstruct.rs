// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzes serialized guardian shards through Shamir key reconstruction.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::{BackupKeyShard, GuardianBackupMetadata, reconstruct_backup_key};

fuzz_target!(|data: &[u8]| {
    let _ = BackupKeyShard::from_bytes(data);

    let shards: Vec<BackupKeyShard> = data
        .chunks_exact(BackupKeyShard::SERIALIZED_LENGTH)
        .take(10)
        .filter_map(|bytes| BackupKeyShard::from_bytes(bytes).ok())
        .collect();
    let _ = reconstruct_backup_key(&shards);

    let mut ceremony_id = [0u8; 16];
    let mut first_value = [0u8; 32];
    let mut second_value = [0u8; 32];
    for (position, byte) in ceremony_id.iter_mut().enumerate() {
        *byte = data.get(position).copied().unwrap_or(position as u8);
    }
    for (position, byte) in first_value.iter_mut().enumerate() {
        *byte = data.get(position + 16).copied().unwrap_or(position as u8);
    }
    for (position, byte) in second_value.iter_mut().enumerate() {
        *byte = data
            .get(position + 48)
            .copied()
            .unwrap_or((position as u8).wrapping_add(1));
    }
    if let Ok(metadata) = GuardianBackupMetadata::new(2, 2, ceremony_id)
        && let (Ok(first), Ok(second)) = (
            BackupKeyShard::from_parts(metadata, 1, first_value),
            BackupKeyShard::from_parts(metadata, 2, second_value),
        )
    {
        let _ = reconstruct_backup_key(&[first, second]);
    }
});
