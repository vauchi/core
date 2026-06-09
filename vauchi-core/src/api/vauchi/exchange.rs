// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange operations: QR code generation for contact exchange.

use crate::contact_card::ContactCard;
use crate::exchange::{ExchangeEvent, ExchangeSession, ManualConfirmationVerifier};

use super::super::error::{VauchiError, VauchiResult};
use super::Vauchi;

/// Exchange QR data with expiration info.
#[derive(Debug, Clone)]
pub struct ExchangeQrData {
    /// The QR code data string.
    pub data: String,
    /// Unix timestamp when the QR was generated.
    pub generated_at: u64,
    /// QR expiration time in seconds.
    pub expires_in_secs: u64,
}

impl ExchangeQrData {
    /// Calculate remaining seconds until expiration relative to `now`.
    ///
    /// `now` is an explicit unix-seconds parameter — callers pass
    /// `vauchi.clock().unix_seconds()` in production and a fixed
    /// value in tests. Phase 1 / Task 1.1 / F3 mop-up: retires the
    /// ambient `SystemTime::now` read this method used to do.
    pub fn remaining_secs(&self, now: u64) -> u64 {
        let expires_at = self.generated_at + self.expires_in_secs;
        expires_at.saturating_sub(now)
    }

    /// Check if the QR code has expired relative to `now`.
    pub fn is_expired(&self, now: u64) -> bool {
        self.remaining_secs(now) == 0
    }
}

impl Vauchi {
    /// Generates exchange QR data for contact exchange.
    ///
    /// Uses ExchangeSession state machine with ManualConfirmationVerifier.
    /// Clones the identity internally since ExchangeSession requires ownership.
    pub fn generate_exchange_qr(&self) -> VauchiResult<ExchangeQrData> {
        let identity = self
            .identity
            .as_ref()
            .ok_or(VauchiError::IdentityNotInitialized)?;

        let our_card = self
            .storage
            .contacts()
            .load_own_card()?
            .unwrap_or_else(|| ContactCard::new(identity.display_name()));

        // Clone identity via storage serialization (ExchangeSession needs ownership)
        let identity_owned = crate::identity::Identity::from_storage_bytes(
            &identity.to_storage_bytes(),
            self.clock.unix_seconds(),
        )
        .map_err(|e| VauchiError::InvalidState(format!("Failed to clone identity: {:?}", e)))?;

        // Create exchange session for mutual QR exchange
        let verifier = ManualConfirmationVerifier::new();
        let mut session =
            ExchangeSession::new_qr(identity_owned, our_card, verifier, self.clock.clone());

        // Generate QR via state machine
        session
            .apply(ExchangeEvent::StartQR)
            .map_err(|e| VauchiError::InvalidState(format!("Failed to generate QR: {:?}", e)))?;

        let qr = session
            .qr()
            .ok_or_else(|| VauchiError::InvalidState("QR code not generated".into()))?;

        Ok(ExchangeQrData {
            data: qr.to_data_string(),
            generated_at: qr.timestamp(),
            expires_in_secs: 300, // 5 minutes, matching QR_EXPIRY_SECONDS
        })
    }
}
