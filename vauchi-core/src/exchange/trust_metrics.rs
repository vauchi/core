// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Trust metrics for auditable trust level derivation.
//!
//! Records every signal that contributed to a contact's trust level.
//! Stored on `Contact` alongside `TrustLevel`.

use serde::{Deserialize, Serialize};

use crate::types::{ExchangeTransport, ProximityConfidence};

/// Inherent proximity guarantee of the transport channel,
/// independent of any additional verifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum TransportProximity {
    /// Physical connection (USB cable) — strongest, no relay possible.
    Physical,
    /// Contact range (NFC tap, 1-4cm) — very strong.
    ContactRange,
    /// Proximate (BLE, up to ~100m) — weak, needs additional verification.
    Proximate,
    /// No inherent proximity (QR display, audio data channel).
    None,
}

impl TransportProximity {
    /// Derives the inherent proximity for a given transport.
    pub fn for_transport(transport: ExchangeTransport) -> Self {
        match transport {
            ExchangeTransport::Usb => Self::Physical,
            ExchangeTransport::Nfc => Self::ContactRange,
            ExchangeTransport::Ble => Self::Proximate,
            // Link mode is asynchronous and relay-mediated — no proximity
            // attestation possible by construction. Group with Qr / Audio
            // (transports without a built-in proximity signal).
            ExchangeTransport::Qr | ExchangeTransport::Audio | ExchangeTransport::Link => {
                Self::None
            }
        }
    }

    /// Whether this proximity level is strong enough for High trust
    /// without additional verifier confirmation.
    pub fn is_strong(&self) -> bool {
        matches!(self, Self::Physical | Self::ContactRange)
    }
}

/// Records every signal that contributed to a contact's trust level.
///
/// Stored on `Contact` alongside `TrustLevel`. Makes trust auditable:
/// "Why is this contact High trust?" → "NFC transport + high proximity
/// confidence."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustMetrics {
    /// Which transport moved the exchange payload.
    pub transport: ExchangeTransport,
    /// Proximity confidence from the verifier.
    pub proximity: ProximityConfidence,
    /// Inherent proximity of the transport itself.
    pub transport_proximity: TransportProximity,
    /// Unix timestamp of the exchange (seconds since epoch).
    pub timestamp: u64,
}

impl TrustMetrics {
    /// Build metrics from exchange results.
    pub fn new(
        transport: ExchangeTransport,
        proximity: ProximityConfidence,
        timestamp: u64,
    ) -> Self {
        let transport_proximity = TransportProximity::for_transport(transport);
        Self {
            transport,
            proximity,
            transport_proximity,
            timestamp,
        }
    }
}
