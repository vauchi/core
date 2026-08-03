// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bounded candidate recovery for guardian backup shards.
//!
//! Recovery keeps at most one surplus ceremony-matching shard, so the worst
//! supported 5-of-10 case tries 462 Shamir subsets. A 128-bit key confirmation
//! filters candidates cheaply; only a matching candidate proceeds to the
//! authoritative backup AEAD open and decompression.
//!
//! This guarantees tolerance of one arbitrary surplus response regardless of
//! ordering. Because sealed responses do not authenticate their sender,
//! multiple matching-metadata injections can still deny recovery by displacing
//! honest responses from the bounded candidate set.

use super::{
    BackupError, BackupKeyShard, FullBackupEnvelope, guardian_backup_key_matches,
    guardian_backup_metadata, import_guardian_backup, reconstruct_backup_key,
};

/// Tries bounded threshold-sized subsets and accepts only an AEAD-authenticated key.
pub(crate) fn recover_from_shards(
    data: &[u8],
    shards: &[BackupKeyShard],
) -> Result<FullBackupEnvelope, BackupError> {
    let metadata = guardian_backup_metadata(data)?;
    let maximum_matching = metadata.count() as usize + 1;
    let mut matching = Vec::with_capacity(maximum_matching);
    for shard in shards.iter().filter(|shard| shard.metadata() == metadata) {
        if !matching.contains(&shard) {
            matching.push(shard);
            if matching.len() == maximum_matching {
                break;
            }
        }
    }
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
            && matches!(
                guardian_backup_key_matches(data, backup_key.symmetric_key()),
                Ok(true)
            )
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
    if indices.is_empty() || item_count < indices.len() {
        return false;
    }
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
    fn one_surplus_response_is_bounded_to_462_candidates() {
        let mut indices: Vec<usize> = (0..5).collect();
        let mut count = 1;
        while advance_combination(&mut indices, 11) {
            count += 1;
        }

        assert_eq!(count, 462);
        assert_eq!(indices, [6, 7, 8, 9, 10]);
    }

    // @internal
    #[test]
    fn combination_iterator_rejects_more_indices_than_items() {
        let mut indices = [0, 1, 2];

        assert!(!advance_combination(&mut indices, 2));
        assert_eq!(indices, [0, 1, 2]);
    }
}
