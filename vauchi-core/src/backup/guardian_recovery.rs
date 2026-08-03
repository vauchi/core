// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded candidate recovery for guardian backup shards.

use super::{
    BackupError, BackupKeyShard, FullBackupEnvelope, guardian_backup_metadata,
    import_guardian_backup, reconstruct_backup_key,
};

/// Tries every threshold-sized subset and accepts only an AEAD-authenticated key.
pub(crate) fn recover_from_shards(
    data: &[u8],
    shards: &[BackupKeyShard],
) -> Result<FullBackupEnvelope, BackupError> {
    let metadata = guardian_backup_metadata(data)?;
    if shards.len() > metadata.count() as usize {
        return Err(BackupError::DecryptionFailed);
    }

    let matching: Vec<&BackupKeyShard> = shards
        .iter()
        .filter(|shard| shard.metadata() == metadata)
        .collect();
    let threshold = metadata.threshold() as usize;
    if matching.len() < threshold {
        return Err(BackupError::DecryptionFailed);
    }

    let mut indices: Vec<usize> = (0..threshold).collect();
    loop {
        let candidate: Vec<BackupKeyShard> = indices
            .iter()
            .map(|&index| matching[index].clone())
            .collect();
        if let Ok(backup_key) = reconstruct_backup_key(&candidate)
            && let Ok(envelope) = import_guardian_backup(data, backup_key.symmetric_key())
        {
            return Ok(envelope);
        }
        if !advance_combination(&mut indices, matching.len()) {
            break;
        }
    }

    Err(BackupError::DecryptionFailed)
}

/// Advances a sorted fixed-size combination over `0..item_count`.
fn advance_combination(indices: &mut [usize], item_count: usize) -> bool {
    for position in (0..indices.len()).rev() {
        let maximum = position + item_count - indices.len();
        if indices[position] < maximum {
            indices[position] += 1;
            for next in position + 1..indices.len() {
                indices[next] = indices[next - 1] + 1;
            }
            return true;
        }
    }
    false
}

// INLINE_TEST_REQUIRED: the combination iterator enforces the recovery work
// bound and is private to this module.
#[cfg(test)]
mod tests {
    use super::*;

    // @internal
    #[test]
    fn ten_choose_five_is_bounded_to_252_candidates() {
        let mut indices: Vec<usize> = (0..5).collect();
        let mut count = 1;
        while advance_combination(&mut indices, 10) {
            count += 1;
        }

        assert_eq!(count, 252);
        assert_eq!(indices, [5, 6, 7, 8, 9]);
    }
}
