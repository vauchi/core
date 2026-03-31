// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange record and trust scoring.
//!
//! [`ExchangeRecord`] captures the full outcome of a single exchange attempt —
//! transport used, proximity signals collected, relay fallback, and context —
//! and derives a [`TrustLevel`] via a two-axis score:
//! `transport_locality × proximity_confidence`.

use serde::{Deserialize, Serialize};

use super::mode::{DataTransport, ExchangeContext, ExchangeMode, ProximityMethod};

// ── ProximityResult ──────────────────────────────────────────────────────────

/// Outcome of a single proximity verification attempt during an exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProximityResult {
    /// The proximity method that was attempted.
    pub method: ProximityMethod,
    /// Confidence in the result, in the range [0.0, 1.0].
    pub confidence: f64,
    /// Whether the verification succeeded.
    pub succeeded: bool,
}

// ── ReverificationRecord ─────────────────────────────────────────────────────

/// A post-exchange proximity check used to re-confirm a contact's presence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverificationRecord {
    /// The proximity method used.
    pub method: ProximityMethod,
    /// Confidence produced by this reverification.
    pub confidence: f64,
    /// Unix timestamp (seconds) when the reverification was performed.
    pub timestamp: u64,
}

// ── ExchangeRecord ───────────────────────────────────────────────────────────

/// Full record of a completed (or attempted) contact exchange.
///
/// Captures transport, context, proximity signals, and relay fallback so that
/// trust can be scored and audited after the fact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExchangeRecord {
    /// Which exchange mode was used.
    pub mode: ExchangeMode,
    /// Physical / logical context of the exchange.
    pub context: ExchangeContext,
    /// Primary data transport channel actually used.
    pub transport_used: DataTransport,
    /// `true` when the exchange fell back to the relay despite a non-relay
    /// primary transport being attempted.
    pub relay_fallback: bool,
    /// All proximity verification results collected during the exchange.
    pub proximity_results: Vec<ProximityResult>,
    /// Unix timestamp (seconds) when the exchange occurred.
    pub timestamp: u64,
    /// Any subsequent reverifications performed after the exchange.
    pub reverifications: Vec<ReverificationRecord>,
}

impl ExchangeRecord {
    /// Compute the two-axis trust score: `transport_locality × proximity_confidence`.
    ///
    /// Both axes are in [0.0, 1.0]; the product is also in [0.0, 1.0].
    pub fn trust_score(&self) -> f64 {
        let locality = self.transport_locality();
        let proximity = self.proximity_confidence();
        locality * proximity
    }

    /// Derive a discrete [`TrustLevel`] from the computed trust score.
    pub fn trust_level(&self) -> TrustLevel {
        TrustLevel::from_score(self.trust_score())
    }

    // ── Private helpers ──────────────────────────────────────────────────────

    /// Transport-axis score (how local / trustworthy the data channel is).
    fn transport_locality(&self) -> f64 {
        match self.transport_used {
            // Direct local channels — full locality.
            DataTransport::QrMultiStage | DataTransport::Ble if !self.relay_fallback => 1.0,
            // Fell back to relay even though a local transport was configured.
            _ if self.relay_fallback => 0.5,
            // Primary relay transport with in-person context (e.g. Web mode).
            DataTransport::Relay if self.context == ExchangeContext::InPerson => 0.5,
            // Primary relay transport for remote / async.
            DataTransport::Relay => 0.3,
            // Catch-all (relay_fallback already handled above).
            _ => 1.0,
        }
    }

    /// Proximity-axis score using diminishing-returns stacking over succeeded results.
    fn proximity_confidence(&self) -> f64 {
        let succeeded: Vec<f64> = self
            .proximity_results
            .iter()
            .filter(|r| r.succeeded)
            .map(|r| r.confidence)
            .collect();

        if succeeded.is_empty() {
            return match self.context {
                ExchangeContext::InPerson => 0.1,
                ExchangeContext::Remote | ExchangeContext::RemoteAsync => 0.0,
            };
        }

        // 1 − ∏(1 − cᵢ)  — diminishing-returns combination.
        let combined = 1.0 - succeeded.iter().fold(1.0_f64, |acc, &c| acc * (1.0 - c));
        combined
    }
}

// ── TrustLevel ───────────────────────────────────────────────────────────────

/// Discrete trust tier derived from a two-axis trust score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustLevel {
    /// Score 0.0 – 0.15: no meaningful verification.
    Lowest,
    /// Score 0.15 – 0.35: remote exchange, no proximity.
    Low,
    /// Score 0.35 – 0.55: in-person but weak proximity signal.
    Medium,
    /// Score 0.55 – 0.75: in-person with proximity verified.
    MediumHigh,
    /// Score 0.75 – 0.90: strong proximity proof.
    High,
    /// Score 0.90 – 1.0: multi-factor verified.
    Highest,
}

impl TrustLevel {
    /// Map a continuous score in [0.0, 1.0] to a discrete trust tier.
    pub fn from_score(score: f64) -> Self {
        if score < 0.15 {
            Self::Lowest
        } else if score < 0.35 {
            Self::Low
        } else if score < 0.55 {
            Self::Medium
        } else if score < 0.75 {
            Self::MediumHigh
        } else if score < 0.90 {
            Self::High
        } else {
            Self::Highest
        }
    }

    /// Short human-readable label for this trust tier.
    pub fn display_text(self) -> &'static str {
        match self {
            Self::Lowest => "Unverified",
            Self::Low => "Remote / unverified proximity",
            Self::Medium => "In person, weak proximity",
            Self::MediumHigh => "In person, proximity verified",
            Self::High => "Strong proximity proof",
            Self::Highest => "Multi-factor verified",
        }
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

// INLINE_TEST_REQUIRED: trust scoring logic depends on private helper methods
// (transport_locality, proximity_confidence) not visible outside this module.
#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(
        mode: ExchangeMode,
        context: ExchangeContext,
        transport: DataTransport,
        relay_fallback: bool,
        proximity: Vec<ProximityResult>,
    ) -> ExchangeRecord {
        ExchangeRecord {
            mode,
            context,
            transport_used: transport,
            relay_fallback,
            proximity_results: proximity,
            timestamp: 1_000_000,
            reverifications: vec![],
        }
    }

    // 1. Link mode: Relay + RemoteAsync + no proximity → score ≈ 0.0
    #[test]
    fn trust_score_link_mode_is_zero() {
        let rec = make_record(
            ExchangeMode::Link,
            ExchangeContext::RemoteAsync,
            DataTransport::Relay,
            false,
            vec![],
        );
        let score = rec.trust_score();
        // transport_locality = 0.3, proximity = 0.0 → 0.0
        assert!((score - 0.0).abs() < 0.01, "expected ~0.0, got {score}");
        assert_eq!(rec.trust_level(), TrustLevel::Lowest);
    }

    // 2. Hover with Audio 0.85: QrMultiStage + InPerson → score ≈ 0.85
    #[test]
    fn trust_score_hover_with_audio() {
        let rec = make_record(
            ExchangeMode::Hover,
            ExchangeContext::InPerson,
            DataTransport::QrMultiStage,
            false,
            vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence: 0.85,
                succeeded: true,
            }],
        );
        let score = rec.trust_score();
        // transport_locality = 1.0, proximity = 0.85 → 0.85
        assert!((score - 0.85).abs() < 0.01, "expected ~0.85, got {score}");
        assert_eq!(rec.trust_level(), TrustLevel::High);
    }

    // 3. relay_fallback=true halves transport locality to 0.5
    #[test]
    fn trust_score_relay_fallback_halves_transport() {
        let rec_no_fallback = make_record(
            ExchangeMode::Hover,
            ExchangeContext::InPerson,
            DataTransport::QrMultiStage,
            false,
            vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence: 0.8,
                succeeded: true,
            }],
        );
        let rec_fallback = make_record(
            ExchangeMode::Hover,
            ExchangeContext::InPerson,
            DataTransport::QrMultiStage,
            true,
            vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence: 0.8,
                succeeded: true,
            }],
        );
        let score_no = rec_no_fallback.trust_score();
        let score_fb = rec_fallback.trust_score();
        // With fallback: 0.5 * 0.8 = 0.4; without: 1.0 * 0.8 = 0.8
        assert!(
            (score_no - 0.8).abs() < 0.01,
            "expected ~0.8, got {score_no}"
        );
        assert!(
            (score_fb - 0.4).abs() < 0.01,
            "expected ~0.4 (half), got {score_fb}"
        );
    }

    // 4. Stacking diminishing returns: NFC(0.95) + Audio(0.85) + Accel(0.5) → > 0.99
    #[test]
    fn trust_score_stacking_diminishing_returns() {
        let rec = make_record(
            ExchangeMode::TapHoverShake,
            ExchangeContext::InPerson,
            DataTransport::Ble,
            false,
            vec![
                ProximityResult {
                    method: ProximityMethod::NfcRange,
                    confidence: 0.95,
                    succeeded: true,
                },
                ProximityResult {
                    method: ProximityMethod::Audio,
                    confidence: 0.85,
                    succeeded: true,
                },
                ProximityResult {
                    method: ProximityMethod::Accelerometer,
                    confidence: 0.50,
                    succeeded: true,
                },
            ],
        );
        let score = rec.trust_score();
        // transport_locality = 1.0
        // proximity = 1 - (1-0.95)*(1-0.85)*(1-0.5) = 1 - 0.05*0.15*0.5 = 1 - 0.00375 ≈ 0.99625
        assert!(score > 0.99, "expected >0.99, got {score}");
        assert_eq!(rec.trust_level(), TrustLevel::Highest);
    }

    // 5. Glance (QrMultiStage + InPerson + no proximity) → base 0.1 × 1.0 = 0.1
    #[test]
    fn trust_score_glance_in_person_base() {
        let rec = make_record(
            ExchangeMode::Glance,
            ExchangeContext::InPerson,
            DataTransport::QrMultiStage,
            false,
            vec![],
        );
        let score = rec.trust_score();
        assert!((score - 0.1).abs() < 0.01, "expected ~0.1, got {score}");
        assert_eq!(rec.trust_level(), TrustLevel::Lowest);
    }

    // 6. Failed proximity results are not counted — falls to base
    #[test]
    fn failed_proximity_not_counted() {
        let rec = make_record(
            ExchangeMode::Hover,
            ExchangeContext::InPerson,
            DataTransport::QrMultiStage,
            false,
            vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence: 0.95,
                succeeded: false, // failed!
            }],
        );
        let score = rec.trust_score();
        // No succeeded results → base 0.1 for InPerson
        assert!(
            (score - 0.1).abs() < 0.01,
            "expected ~0.1 (base), got {score}"
        );
    }

    // 7. ExchangeRecord serde roundtrip
    #[test]
    fn serde_roundtrip() {
        let rec = ExchangeRecord {
            mode: ExchangeMode::TapTap,
            context: ExchangeContext::InPerson,
            transport_used: DataTransport::Ble,
            relay_fallback: false,
            proximity_results: vec![ProximityResult {
                method: ProximityMethod::NfcRange,
                confidence: 0.9,
                succeeded: true,
            }],
            timestamp: 1_700_000_000,
            reverifications: vec![ReverificationRecord {
                method: ProximityMethod::Audio,
                confidence: 0.7,
                timestamp: 1_700_000_600,
            }],
        };
        let json = serde_json::to_string(&rec).expect("serialize");
        let back: ExchangeRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.mode, rec.mode);
        assert_eq!(back.context, rec.context);
        assert_eq!(back.transport_used, rec.transport_used);
        assert_eq!(back.relay_fallback, rec.relay_fallback);
        assert_eq!(back.timestamp, rec.timestamp);
        assert_eq!(back.proximity_results.len(), 1);
        assert_eq!(back.reverifications.len(), 1);
        // Trust score is deterministic
        assert!((back.trust_score() - rec.trust_score()).abs() < 0.001);
    }

    // 8. TrustLevel threshold boundaries
    #[test]
    fn trust_level_from_score() {
        assert_eq!(TrustLevel::from_score(0.0), TrustLevel::Lowest);
        assert_eq!(TrustLevel::from_score(0.14), TrustLevel::Lowest);
        assert_eq!(TrustLevel::from_score(0.15), TrustLevel::Low);
        assert_eq!(TrustLevel::from_score(0.34), TrustLevel::Low);
        assert_eq!(TrustLevel::from_score(0.35), TrustLevel::Medium);
        assert_eq!(TrustLevel::from_score(0.54), TrustLevel::Medium);
        assert_eq!(TrustLevel::from_score(0.55), TrustLevel::MediumHigh);
        assert_eq!(TrustLevel::from_score(0.74), TrustLevel::MediumHigh);
        assert_eq!(TrustLevel::from_score(0.75), TrustLevel::High);
        assert_eq!(TrustLevel::from_score(0.89), TrustLevel::High);
        assert_eq!(TrustLevel::from_score(0.90), TrustLevel::Highest);
        assert_eq!(TrustLevel::from_score(1.0), TrustLevel::Highest);
    }
}
