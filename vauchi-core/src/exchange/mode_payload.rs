// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Bootstrap message payload for exchange mode negotiation.
//!
//! [`ExchangeModePayload`] is the structured message exchanged at the start of
//! a session to announce which [`ExchangeMode`] the sender is proposing, what
//! [`DeviceCapabilities`] it has, the unique [`ExchangeId`] for the session,
//! and a TTL so stale payloads can be discarded.

use serde::{Deserialize, Serialize};

use crate::exchange::capability::types::DeviceCapabilities;
use crate::exchange::exchange_id::ExchangeId;
use crate::exchange::mode::ExchangeMode;

// ── Payload type ─────────────────────────────────────────────────────────────

/// Bootstrap payload included in the first message of an exchange session.
///
/// Sent by the initiator to declare the proposed mode, the device's
/// capabilities (so the peer can validate compatibility), the session ID, and
/// timing information used to detect stale or replayed payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeModePayload {
    /// The exchange mode the sender is proposing.
    pub mode: ExchangeMode,
    /// Hardware capabilities reported by the sender's device.
    pub capabilities: DeviceCapabilities,
    /// Unique identifier for this exchange session.
    pub exchange_id: ExchangeId,
    /// Unix timestamp (seconds) when this payload was created.
    pub timestamp: u64,
    /// How many seconds this payload should be considered valid.
    pub ttl_seconds: u32,
}

impl ExchangeModePayload {
    /// Returns `true` if the payload's mode matches `expected`.
    ///
    /// Used by the responder to verify the peer is using the same mode.
    pub fn is_mode_compatible(&self, expected: ExchangeMode) -> bool {
        self.mode == expected
    }

    /// Whether this payload has expired.
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs > self.timestamp.saturating_add(u64::from(self.ttl_seconds))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: tests verify serde roundtrip fidelity for ExchangeModePayload
// including ExchangeId hex serialization which is not visible from outside the module.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::capability::types::DeviceCapabilities;
    use crate::exchange::exchange_id::ExchangeId;
    use crate::exchange::mode::ExchangeMode;
    use crate::types::AudioCapability;

    fn sample_payload() -> ExchangeModePayload {
        ExchangeModePayload {
            mode: ExchangeMode::Hover,
            capabilities: DeviceCapabilities {
                has_camera: true,
                has_ble: true,
                has_nfc: false,
                audio: AudioCapability::Full,
                has_accelerometer: true,
                has_internet: true,
                ..Default::default()
            },
            exchange_id: ExchangeId::generate(),
            timestamp: 1_700_000_000,
            ttl_seconds: 60,
        }
    }

    #[test]
    fn serde_roundtrip() {
        let payload = sample_payload();
        let original_id = payload.exchange_id;
        let original_mode = payload.mode;

        let json = serde_json::to_string(&payload).expect("serialize");
        let decoded: ExchangeModePayload = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(decoded.mode, original_mode);
        assert_eq!(decoded.exchange_id, original_id);
        assert_eq!(decoded.timestamp, 1_700_000_000);
        assert_eq!(decoded.ttl_seconds, 60);
        assert!(decoded.capabilities.has_camera);
        assert!(decoded.capabilities.has_ble);
        assert!(!decoded.capabilities.has_nfc);
    }

    #[test]
    fn is_compatible_same_mode() {
        let payload = sample_payload(); // mode = Hover

        assert!(
            payload.is_mode_compatible(ExchangeMode::Hover),
            "same mode must be compatible"
        );
        assert!(
            !payload.is_mode_compatible(ExchangeMode::Glance),
            "different mode must not be compatible"
        );
        assert!(
            !payload.is_mode_compatible(ExchangeMode::Link),
            "different mode must not be compatible"
        );
    }

    #[test]
    fn is_expired_respects_ttl() {
        let payload = ExchangeModePayload {
            mode: ExchangeMode::Hover,
            capabilities: DeviceCapabilities::default(),
            exchange_id: ExchangeId::generate(),
            timestamp: 1000,
            ttl_seconds: 60,
        };
        assert!(!payload.is_expired(1059)); // within TTL
        assert!(payload.is_expired(1061)); // past TTL
    }

    #[test]
    fn zero_ttl_immediately_expired() {
        let payload = ExchangeModePayload {
            mode: ExchangeMode::Link,
            capabilities: DeviceCapabilities::default(),
            exchange_id: ExchangeId::generate(),
            timestamp: 1000,
            ttl_seconds: 0,
        };
        assert!(payload.is_expired(1001));
    }
}
