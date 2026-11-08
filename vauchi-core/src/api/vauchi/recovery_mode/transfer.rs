// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

use super::ProfileEngine;
use crate::api::error::{VauchiError, VauchiResult};
use crate::sync::guardian_transfer::HandBackReceiver;

impl ProfileEngine {
    /// Accepts one recovery transport packet and returns its flow-control ACK.
    /// A completed piece is stored and the whole-backup hash checked before
    /// its final ACK can escape (Social Recovery A11, P33).
    ///
    /// The Core ceremony driver owns this receiver and its scoped BLE link;
    /// shells report observations through the generic command/event boundary.
    /// On cancellation the driver must cancel the receiver and stop OS work.
    ///
    /// # Errors
    /// A single sanitized refusal covers missing recovery state or accepted
    /// share, malformed packets, cancellation, storage and backup validation.
    /// Retrying the final packet can retry an unsuccessful durable write.
    pub fn take_hand_back_frame(
        &self,
        receiver: &mut HandBackReceiver,
        frame: &[u8],
    ) -> VauchiResult<Vec<u8>> {
        let refused = || VauchiError::InvalidState("Recovery transfer could not continue".into());
        self.recovery_mode_seed().map_err(|_| refused())?;
        if !self
            .hand_back_shares()
            .map_err(|_| refused())?
            .iter()
            .any(|share| share.backup_hash() == receiver.backup_hash())
        {
            return Err(refused());
        }
        let receipt = receiver.receive(frame).map_err(|_| refused())?;
        if let Some(piece) = receipt.piece() {
            self.take_hand_back_piece(&piece.encode())
                .map_err(|_| refused())?;
        }
        Ok(receipt.acknowledgement().to_vec())
    }
}
