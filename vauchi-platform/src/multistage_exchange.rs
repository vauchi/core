// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for the multi-stage exchange session.
//!
//! Wraps [`MultiStageSession`] in a thread-safe handle (`Mutex`)
//! and exposes mobile-friendly enums/records for state and QR payloads.

use std::sync::Mutex;

use crate::error::LOCK_POISON_MSG;

use vauchi_core::exchange::{MultiStageSession, ProtocolState, QrPayload};

/// Mobile-friendly protocol state enum (UniFFI-compatible).
#[derive(uniffi::Enum, Debug, Clone, PartialEq)]
pub enum MobileProtocolState {
    Idle,
    Advertising,
    Discovered,
    Transferring {
        chunks_sent: u16,
        chunks_total: u16,
        chunks_received: u16,
        peer_chunks_total: u16,
    },
    Verifying,
    Confirming,
    Complete,
    Finalized,
    Failed {
        reason: String,
    },
}

impl From<ProtocolState> for MobileProtocolState {
    fn from(state: ProtocolState) -> Self {
        match state {
            ProtocolState::Idle => MobileProtocolState::Idle,
            ProtocolState::Advertising => MobileProtocolState::Advertising,
            ProtocolState::Discovered => MobileProtocolState::Discovered,
            ProtocolState::Transferring {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            } => MobileProtocolState::Transferring {
                chunks_sent,
                chunks_total,
                chunks_received,
                peer_chunks_total,
            },
            ProtocolState::Verifying => MobileProtocolState::Verifying,
            ProtocolState::Confirming => MobileProtocolState::Confirming,
            ProtocolState::Complete | ProtocolState::RetryReady => MobileProtocolState::Complete,
            ProtocolState::Finalized => MobileProtocolState::Finalized,
            ProtocolState::Failed(reason) => MobileProtocolState::Failed { reason },
            _ => MobileProtocolState::Idle,
        }
    }
}

/// QR payload for mobile display.
#[derive(uniffi::Record, Debug, Clone)]
pub struct MobileQrPayload {
    pub data: String,
    pub error_correction: String,
    pub display_duration_ms: u32,
}

impl From<QrPayload> for MobileQrPayload {
    fn from(qr: QrPayload) -> Self {
        MobileQrPayload {
            data: qr.data,
            error_correction: qr.error_correction,
            display_duration_ms: qr.display_duration_ms,
        }
    }
}

/// Multi-stage exchange session handle for mobile platforms.
#[derive(uniffi::Object)]
pub struct MobileMultiStageSession {
    inner: Mutex<MultiStageSession>,
}

#[uniffi::export]
impl MobileMultiStageSession {
    /// Create a new session with the local contact card to share.
    #[uniffi::constructor]
    pub fn new(local_card: Vec<u8>) -> Self {
        MobileMultiStageSession {
            inner: Mutex::new(MultiStageSession::new(local_card)),
        }
    }

    /// Get the QR payload the app should display right now.
    pub fn get_display_qr(&self) -> Option<MobileQrPayload> {
        let Ok(mut session) = self.inner.lock() else {
            return None;
        };
        session.get_display_qr().map(MobileQrPayload::from)
    }

    /// Feed a scanned QR string into the protocol engine.
    pub fn process_scanned_qr(&self, raw: String) -> MobileProtocolState {
        let Ok(mut session) = self.inner.lock() else {
            return MobileProtocolState::Failed {
                reason: LOCK_POISON_MSG.into(),
            };
        };
        session.process_scanned_qr(&raw).into()
    }

    /// Poll current state.
    pub fn get_state(&self) -> MobileProtocolState {
        let Ok(session) = self.inner.lock() else {
            return MobileProtocolState::Failed {
                reason: LOCK_POISON_MSG.into(),
            };
        };
        session.get_state().into()
    }

    /// On Complete: retrieve the peer's decrypted contact card.
    pub fn get_received_data(&self) -> Option<Vec<u8>> {
        let Ok(session) = self.inner.lock() else {
            return None;
        };
        session.get_received_data()
    }

    /// Returns the ECDH transport key established during the exchange.
    ///
    /// Used by `VauchiPlatform::finalize_multistage_exchange` to derive
    /// the shared secret for the double ratchet.
    pub fn get_transport_key(&self) -> Option<Vec<u8>> {
        let Ok(session) = self.inner.lock() else {
            return None;
        };
        session.get_transport_key().map(|k| k.to_vec())
    }

    /// Abort and wipe session.
    pub fn cancel(&self) {
        let Ok(mut session) = self.inner.lock() else {
            return;
        };
        session.cancel();
    }
}
