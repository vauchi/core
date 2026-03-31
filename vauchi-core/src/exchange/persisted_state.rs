// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Persisted exchange state for crash recovery.
//!
//! [`PersistedExchangeState`] is written to durable storage at each lifecycle
//! transition so that an interrupted exchange can be resumed or safely cleaned
//! up after a crash or unexpected app termination.

use serde::{Deserialize, Serialize};

use super::exchange_id::ExchangeId;
use super::exchange_record::ProximityResult;
use super::mode::ExchangeMode;

// ── ExchangeLifecycleState ───────────────────────────────────────────────────

/// Discrete phase of an in-progress exchange session.
///
/// Variants are ordered by `repr(u8)` so that phase ordering can be compared
/// with integer comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ExchangeLifecycleState {
    /// Data transfer is in progress (QR scanning, BLE pairing, etc.).
    Transferring = 0,
    /// Both card payloads have been received; waiting for commitment.
    Committing = 1,
    /// Contact record is being written to storage.
    Finalizing = 2,
    /// Exchange finished successfully.
    Complete = 3,
}

// ── PersistedExchangeState ───────────────────────────────────────────────────

/// Durable snapshot of an ongoing exchange session.
///
/// Written to crash-safe storage after each lifecycle transition.
/// Escrow fields are only populated for modes that use relay escrow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedExchangeState {
    /// Unique identifier for this exchange session.
    pub exchange_id: ExchangeId,
    /// Which exchange mode is being used.
    pub mode: ExchangeMode,
    /// Current lifecycle phase.
    pub state: ExchangeLifecycleState,
    /// Relay escrow gate hash (32 bytes). Length enforced at
    /// construction by the escrow client (Tier 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrow_gate_hash: Option<Vec<u8>>,
    /// Our encrypted escrow slot handle (32 bytes). Length enforced at
    /// construction by the escrow client (Tier 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrow_our_slot: Option<Vec<u8>>,
    /// Their encrypted escrow slot handle (32 bytes). Length enforced at
    /// construction by the escrow client (Tier 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escrow_their_slot: Option<Vec<u8>>,
    /// Their encrypted contact card blob (set once received).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub their_encrypted_card: Option<Vec<u8>>,
    /// Proximity verification results collected so far.
    pub proximity_results: Vec<ProximityResult>,
    /// Unix timestamp (seconds) when the exchange was initiated.
    pub created_at: u64,
    /// How long (seconds) before this persisted state is considered expired.
    pub ttl_seconds: u32,
}

impl PersistedExchangeState {
    /// Returns `true` if the state has expired at the given wall-clock time.
    ///
    /// Expiry is defined as: `now_secs > created_at + ttl_seconds`.
    pub fn is_expired(&self, now_secs: u64) -> bool {
        now_secs > self.created_at.saturating_add(u64::from(self.ttl_seconds))
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: ExchangeLifecycleState repr(u8) ordering and
// PersistedExchangeState TTL logic are private implementation details not
// visible outside this module.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::mode::ProximityMethod;

    // 1. repr(u8) ordering: Transferring < Committing < Finalizing < Complete
    #[test]
    fn lifecycle_states_are_ordered() {
        assert!(
            (ExchangeLifecycleState::Transferring as u8)
                < (ExchangeLifecycleState::Committing as u8)
        );
        assert!(
            (ExchangeLifecycleState::Committing as u8) < (ExchangeLifecycleState::Finalizing as u8)
        );
        assert!(
            (ExchangeLifecycleState::Finalizing as u8) < (ExchangeLifecycleState::Complete as u8)
        );
        assert_eq!(ExchangeLifecycleState::Transferring as u8, 0);
        assert_eq!(ExchangeLifecycleState::Committing as u8, 1);
        assert_eq!(ExchangeLifecycleState::Finalizing as u8, 2);
        assert_eq!(ExchangeLifecycleState::Complete as u8, 3);
    }

    // 2. is_expired respects TTL
    #[test]
    fn is_expired_respects_ttl() {
        let state = PersistedExchangeState {
            exchange_id: ExchangeId::from_bytes([0u8; 32]),
            mode: ExchangeMode::Hover,
            state: ExchangeLifecycleState::Transferring,
            escrow_gate_hash: None,
            escrow_our_slot: None,
            escrow_their_slot: None,
            their_encrypted_card: None,
            proximity_results: vec![],
            created_at: 1_000_000,
            ttl_seconds: 300,
        };
        // Not yet expired: exactly at boundary
        assert!(!state.is_expired(1_000_300));
        // Expired: one second past boundary
        assert!(state.is_expired(1_000_301));
        // Definitely not expired: before creation
        assert!(!state.is_expired(999_999));
    }

    // 3. Serde roundtrip with escrow fields populated
    #[test]
    fn serde_roundtrip() {
        let state = PersistedExchangeState {
            exchange_id: ExchangeId::from_bytes([42u8; 32]),
            mode: ExchangeMode::Link,
            state: ExchangeLifecycleState::Committing,
            escrow_gate_hash: Some(vec![1, 2, 3, 4]),
            escrow_our_slot: Some(vec![5, 6, 7, 8]),
            escrow_their_slot: Some(vec![9, 10, 11, 12]),
            their_encrypted_card: Some(vec![0xca, 0xfe]),
            proximity_results: vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence: 0.75,
                succeeded: true,
            }],
            created_at: 1_700_000_000,
            ttl_seconds: 604_800, // 7 days
        };

        let json = serde_json::to_string(&state).expect("serialize");
        let back: PersistedExchangeState = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.mode, state.mode);
        assert_eq!(back.state, state.state);
        assert_eq!(back.escrow_gate_hash, state.escrow_gate_hash);
        assert_eq!(back.escrow_our_slot, state.escrow_our_slot);
        assert_eq!(back.escrow_their_slot, state.escrow_their_slot);
        assert_eq!(back.their_encrypted_card, state.their_encrypted_card);
        assert_eq!(back.created_at, state.created_at);
        assert_eq!(back.ttl_seconds, state.ttl_seconds);
        assert_eq!(back.proximity_results.len(), 1);
        assert_eq!(back.proximity_results[0].method, ProximityMethod::Audio);
        // ExchangeId should survive the roundtrip
        assert_eq!(back.exchange_id.as_bytes(), state.exchange_id.as_bytes());
    }
}
