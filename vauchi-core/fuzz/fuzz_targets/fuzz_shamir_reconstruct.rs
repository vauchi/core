// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzes serialized guardian shards through Shamir key reconstruction.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::{BackupKeyShard, reconstruct_backup_key};

const SHARD_WIRE_LENGTH: usize = 52;

fuzz_target!(|data: &[u8]| {
    let _ = BackupKeyShard::from_bytes(data);

    let shards: Vec<BackupKeyShard> = data
        .chunks_exact(SHARD_WIRE_LENGTH)
        .take(10)
        .filter_map(|bytes| BackupKeyShard::from_bytes(bytes).ok())
        .collect();
    let _ = reconstruct_backup_key(&shards);
});
