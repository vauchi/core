// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The receive kinds that apply no card change: cover (0x09,
//! vauchi/private#811), a resync request (0x0a, #691), a guardian backup
//! piece (0x0b), a guardian release (0x0c), a returned backup piece
//! (0x0d) and a recovery continuation (0x0e, #796).
//!
//! The ratchet is persisted as for an update, so the sender's next message
//! needs no skipped key and a deferred sender is released the same way;
//! none may end as an error, which would keep the ratchet unsaved and count
//! as a failure. A piece is kept and a release honoured in the same
//! transaction, so a storage failure leaves the blob on the relay;
//! honouring a resync request is the receive loop's.

use super::{CardUpdateError, ReceiveOutcome};
use crate::crypto::ratchet::DoubleRatchetState;
use crate::storage::Storage;
use crate::sync::delta::{DeltaError, VersionedPayload};

/// The outcome of `payload` if it is one of the kinds above.
pub(super) fn outcome_of(payload: Result<VersionedPayload, DeltaError>) -> Option<ReceiveOutcome> {
    match payload {
        Ok(VersionedPayload::Cover) => Some(ReceiveOutcome::Cover),
        Ok(VersionedPayload::ResyncRequest) => Some(ReceiveOutcome::ResyncRequest),
        Ok(VersionedPayload::GuardianBackupPiece(piece)) => {
            Some(ReceiveOutcome::GuardianBackupPiece(piece))
        }
        Ok(VersionedPayload::GuardianRelease) => Some(ReceiveOutcome::GuardianRelease),
        Ok(VersionedPayload::ReturnedBackupPiece(piece)) => {
            Some(ReceiveOutcome::ReturnedBackupPiece(piece))
        }
        Ok(VersionedPayload::RecoveryContinuation(continuation)) => {
            Some(ReceiveOutcome::RecoveryContinuation(continuation))
        }
        _ => None,
    }
}

/// Persists the ratchet and does what `outcome` asks of storage, in one
/// transaction.
pub(super) fn receive(
    storage: &Storage,
    sender_id: &str,
    peer_device_id: &[u8; 32],
    ratchet: &DoubleRatchetState,
    is_initiator: bool,
    outcome: ReceiveOutcome,
) -> Result<ReceiveOutcome, CardUpdateError> {
    storage.begin_transaction()?;
    let now = storage.clock().unix_seconds();
    let txn = storage
        .ratchets()
        .save_ratchet_state_for_device(sender_id, peer_device_id, ratchet, is_initiator)
        .and_then(|()| match &outcome {
            ReceiveOutcome::GuardianBackupPiece(piece) => storage
                .guardian_backups()
                .keep_piece(sender_id, piece, now)
                .map(|_| ()),
            ReceiveOutcome::GuardianRelease => {
                storage.guardian_backups().forget_designator(sender_id)
            }
            ReceiveOutcome::ReturnedBackupPiece(piece) => storage
                .returned_backups()
                .keep_piece(sender_id, piece, now)
                .map(|_| ()),
            ReceiveOutcome::RecoveryContinuation(continuation) => storage
                .honour_recovery_continuation(sender_id, continuation)
                .map(|_| ()),
            _ => Ok(()),
        });
    match txn {
        Ok(()) => storage.commit()?,
        Err(e) => {
            storage.rollback();
            return Err(e.into());
        }
    }
    Ok(outcome)
}
