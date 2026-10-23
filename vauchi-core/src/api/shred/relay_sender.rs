// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::network::{PurgeRequest, RelayClient, Transport};

impl<T: Transport> crate::api::PurgeSender for RelayClient<T> {
    fn send_purge(
        &mut self,
        purge: &crate::api::PreSignedPurgeRequest,
        now: u64,
    ) -> Result<bool, crate::api::ShredError> {
        let request = PurgeRequest {
            public_key: crate::identifiers::IdentityKey::from(purge.public_key),
            signature: purge.signature.clone(),
            purge_token: purge.purge_token,
            timestamp: purge.timestamp,
        };

        match self.send_purge_request(&request, now) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay purge failed: {}",
                e
            ))),
        }
    }
}

impl<T: Transport> crate::api::RevocationSender for RelayClient<T> {
    fn send_revocation_delivery(
        &mut self,
        token: &str,
        blob_b64: &str,
        now: u64,
    ) -> Result<bool, crate::api::ShredError> {
        use base64::Engine;
        let ciphertext = base64::engine::general_purpose::STANDARD
            .decode(blob_b64)
            .map_err(|e| {
                crate::api::ShredError::FileError(format!("invalid revocation blob: {e}"))
            })?;
        match self.send_revocation_blob(token, ciphertext, now) {
            Ok(_) => Ok(true),
            Err(e) => Err(crate::api::ShredError::FileError(format!(
                "Relay revocation failed: {e}"
            ))),
        }
    }
}
