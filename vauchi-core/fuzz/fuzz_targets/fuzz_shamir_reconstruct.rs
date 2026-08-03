// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Fuzzes serialized guardian shards through Shamir key reconstruction.

#![no_main]

use libfuzzer_sys::fuzz_target;
use vauchi_core::{BackupKeyShard, GuardianBackupMetadata, reconstruct_backup_key};

fn gf_mul_reference(mut left: u8, mut right: u8) -> u8 {
    let mut product = 0u8;
    while right != 0 {
        if right & 1 != 0 {
            product ^= left;
        }
        let carry = left & 0x80;
        left <<= 1;
        if carry != 0 {
            left ^= 0x1b;
        }
        right >>= 1;
    }
    product
}

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
        let mut expected = [0u8; 32];
        for position in 0..32 {
            // For x=1 and x=2, the Lagrange coefficients at zero are
            // 2/3 = 0xf7 and 1/3 = 0xf6 in the AES field.
            expected[position] = gf_mul_reference(first_value[position], 0xf7)
                ^ gf_mul_reference(second_value[position], 0xf6);
        }
        match reconstruct_backup_key(&[first, second]) {
            Ok(reconstructed) => assert_eq!(reconstructed.as_bytes(), &expected),
            Err(_) => assert!(expected.iter().all(|byte| *byte == 0)),
        }
    }
});
