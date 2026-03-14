// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Proximity Verifier Event System
//!
//! Observable events emitted during proximity verification, allowing
//! platform/UI layers to show meaningful progress and fallback status.

use super::ProximityConfidence;
use serde::Serialize;

/// Identifies which proximity verification method is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifierMethod {
    Ultrasonic,
    AmbientAudio,
    Accelerometer,
    ManualConfirmation,
    Nfc,
    Ble,
}

impl VerifierMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ultrasonic => "ultrasonic",
            Self::AmbientAudio => "ambient_audio",
            Self::Accelerometer => "accelerometer",
            Self::ManualConfirmation => "manual_confirmation",
            Self::Nfc => "nfc",
            Self::Ble => "ble",
        }
    }
}

/// Events emitted during proximity verification.
///
/// The platform/UI layer observes these to update the user interface:
/// - Show instructions ("Tap the table with both phones")
/// - Show progress bars
/// - Indicate method failures and fallbacks
/// - Announce completion with confidence level
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProximityVerifierEvent {
    /// Waiting for user action before verification can proceed.
    /// e.g., "Hold phones together" for accelerometer.
    WaitingForAction {
        method: VerifierMethod,
        instruction: String,
    },

    /// Verification is in progress for the given method.
    InProgress {
        method: VerifierMethod,
        /// 0–100 percent.
        progress_pct: u8,
    },

    /// A verification method failed.
    MethodFailed {
        method: VerifierMethod,
        reason: String,
    },

    /// Falling back from one method to another.
    FallingBack {
        failed_method: VerifierMethod,
        next_method: VerifierMethod,
    },

    /// Verification completed successfully.
    Completed {
        method: VerifierMethod,
        confidence: ProximityConfidence,
    },

    /// All available methods have been tried and failed.
    AllMethodsExhausted,
}

/// Ordered log of verifier events for a single verification attempt.
///
/// Used by the VerifierChain to track progress and by tests to assert
/// the correct sequence of events.
#[derive(Debug, Default)]
pub struct VerifierEventLog {
    events: Vec<ProximityVerifierEvent>,
}

impl VerifierEventLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: ProximityVerifierEvent) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[ProximityVerifierEvent] {
        &self.events
    }

    pub fn last(&self) -> Option<&ProximityVerifierEvent> {
        self.events.last()
    }

    /// Whether verification completed successfully.
    pub fn is_completed(&self) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, ProximityVerifierEvent::Completed { .. }))
    }

    /// Whether all methods were exhausted without success.
    pub fn is_exhausted(&self) -> bool {
        self.events
            .iter()
            .any(|e| matches!(e, ProximityVerifierEvent::AllMethodsExhausted))
    }

    /// The final confidence level, if verification completed.
    pub fn final_confidence(&self) -> Option<ProximityConfidence> {
        self.events.iter().rev().find_map(|e| match e {
            ProximityVerifierEvent::Completed { confidence, .. } => Some(*confidence),
            _ => None,
        })
    }
}
