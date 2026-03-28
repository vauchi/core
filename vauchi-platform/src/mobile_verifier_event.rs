// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Mobile bindings for proximity verifier events.
//!
//! Wraps vauchi-core's `ProximityVerifierEvent` and related types for
//! UniFFI export to iOS/Android platforms. These types drive the
//! proximity verification UI (progress bars, method fallback indicators,
//! completion status).

use vauchi_core::exchange::{ProximityConfidence, ProximityVerifierEvent, VerifierMethod};

/// Mobile-friendly proximity confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileProximityConfidence {
    High,
    Medium,
    Low,
    Unknown,
}

impl From<ProximityConfidence> for MobileProximityConfidence {
    fn from(c: ProximityConfidence) -> Self {
        match c {
            ProximityConfidence::High => Self::High,
            ProximityConfidence::Medium => Self::Medium,
            ProximityConfidence::Low => Self::Low,
            ProximityConfidence::Unknown => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

/// Identifies which proximity verification method is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MobileVerifierMethod {
    Ultrasonic,
    AmbientAudio,
    Accelerometer,
    ManualConfirmation,
    Nfc,
    Ble,
}

impl MobileVerifierMethod {
    /// Human-readable label for display in the UI.
    pub fn display_label(&self) -> &'static str {
        match self {
            Self::Ultrasonic => "Ultrasonic",
            Self::AmbientAudio => "Ambient Audio",
            Self::Accelerometer => "Accelerometer",
            Self::ManualConfirmation => "Manual Confirmation",
            Self::Nfc => "NFC",
            Self::Ble => "Bluetooth",
        }
    }
}

impl From<VerifierMethod> for MobileVerifierMethod {
    fn from(m: VerifierMethod) -> Self {
        match m {
            VerifierMethod::Ultrasonic => Self::Ultrasonic,
            VerifierMethod::AmbientAudio => Self::AmbientAudio,
            VerifierMethod::Accelerometer => Self::Accelerometer,
            VerifierMethod::ManualConfirmation => Self::ManualConfirmation,
            VerifierMethod::Nfc => Self::Nfc,
            VerifierMethod::Ble => Self::Ble,
            _ => Self::ManualConfirmation,
        }
    }
}

/// Events emitted during proximity verification for mobile UI consumption.
///
/// The mobile platform layer observes these to update the user interface:
/// - Show instructions ("Tap the table with both phones")
/// - Show progress bars
/// - Indicate method failures and fallbacks
/// - Announce completion with confidence level
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileProximityVerifierEvent {
    /// Waiting for user action before verification can proceed.
    WaitingForAction {
        method: MobileVerifierMethod,
        instruction: String,
    },

    /// Verification is in progress for the given method.
    InProgress {
        method: MobileVerifierMethod,
        /// 0–100 percent.
        progress_pct: u8,
    },

    /// A verification method failed.
    MethodFailed {
        method: MobileVerifierMethod,
        reason: String,
    },

    /// Falling back from one method to another.
    FallingBack {
        failed_method: MobileVerifierMethod,
        next_method: MobileVerifierMethod,
    },

    /// Verification completed successfully.
    Completed {
        method: MobileVerifierMethod,
        confidence: MobileProximityConfidence,
    },

    /// All available methods have been tried and failed.
    AllMethodsExhausted,
}

impl From<ProximityVerifierEvent> for MobileProximityVerifierEvent {
    fn from(event: ProximityVerifierEvent) -> Self {
        match event {
            ProximityVerifierEvent::WaitingForAction {
                method,
                instruction,
            } => Self::WaitingForAction {
                method: method.into(),
                instruction,
            },
            ProximityVerifierEvent::InProgress {
                method,
                progress_pct,
            } => Self::InProgress {
                method: method.into(),
                progress_pct,
            },
            ProximityVerifierEvent::MethodFailed { method, reason } => Self::MethodFailed {
                method: method.into(),
                reason,
            },
            ProximityVerifierEvent::FallingBack {
                failed_method,
                next_method,
            } => Self::FallingBack {
                failed_method: failed_method.into(),
                next_method: next_method.into(),
            },
            ProximityVerifierEvent::Completed { method, confidence } => Self::Completed {
                method: method.into(),
                confidence: confidence.into(),
            },
            ProximityVerifierEvent::AllMethodsExhausted => Self::AllMethodsExhausted,
            _ => Self::AllMethodsExhausted,
        }
    }
}

// INLINE_TEST_REQUIRED: tests use From conversions on private types
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confidence_high_converts() {
        let mobile: MobileProximityConfidence = ProximityConfidence::High.into();
        assert_eq!(mobile, MobileProximityConfidence::High);
    }

    #[test]
    fn confidence_medium_converts() {
        let mobile: MobileProximityConfidence = ProximityConfidence::Medium.into();
        assert_eq!(mobile, MobileProximityConfidence::Medium);
    }

    #[test]
    fn confidence_low_converts() {
        let mobile: MobileProximityConfidence = ProximityConfidence::Low.into();
        assert_eq!(mobile, MobileProximityConfidence::Low);
    }

    #[test]
    fn confidence_unknown_converts() {
        let mobile: MobileProximityConfidence = ProximityConfidence::Unknown.into();
        assert_eq!(mobile, MobileProximityConfidence::Unknown);
    }

    #[test]
    fn method_ultrasonic_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::Ultrasonic.into();
        assert_eq!(mobile, MobileVerifierMethod::Ultrasonic);
    }

    #[test]
    fn method_ambient_audio_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::AmbientAudio.into();
        assert_eq!(mobile, MobileVerifierMethod::AmbientAudio);
    }

    #[test]
    fn method_accelerometer_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::Accelerometer.into();
        assert_eq!(mobile, MobileVerifierMethod::Accelerometer);
    }

    #[test]
    fn method_manual_confirmation_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::ManualConfirmation.into();
        assert_eq!(mobile, MobileVerifierMethod::ManualConfirmation);
    }

    #[test]
    fn method_nfc_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::Nfc.into();
        assert_eq!(mobile, MobileVerifierMethod::Nfc);
    }

    #[test]
    fn method_ble_converts() {
        let mobile: MobileVerifierMethod = VerifierMethod::Ble.into();
        assert_eq!(mobile, MobileVerifierMethod::Ble);
    }

    #[test]
    fn display_labels_readable() {
        assert_eq!(
            MobileVerifierMethod::Ultrasonic.display_label(),
            "Ultrasonic"
        );
        assert_eq!(
            MobileVerifierMethod::AmbientAudio.display_label(),
            "Ambient Audio"
        );
        assert_eq!(
            MobileVerifierMethod::Accelerometer.display_label(),
            "Accelerometer"
        );
        assert_eq!(
            MobileVerifierMethod::ManualConfirmation.display_label(),
            "Manual Confirmation"
        );
        assert_eq!(MobileVerifierMethod::Nfc.display_label(), "NFC");
        assert_eq!(MobileVerifierMethod::Ble.display_label(), "Bluetooth");
    }

    #[test]
    fn event_waiting_for_action_converts() {
        let event = ProximityVerifierEvent::WaitingForAction {
            method: VerifierMethod::Accelerometer,
            instruction: "Tap the table".to_string(),
        };
        let mobile: MobileProximityVerifierEvent = event.into();
        match mobile {
            MobileProximityVerifierEvent::WaitingForAction {
                method,
                instruction,
            } => {
                assert_eq!(method, MobileVerifierMethod::Accelerometer);
                assert_eq!(instruction, "Tap the table");
            }
            other => panic!("Expected WaitingForAction, got {:?}", other),
        }
    }

    #[test]
    fn event_in_progress_converts() {
        let event = ProximityVerifierEvent::InProgress {
            method: VerifierMethod::AmbientAudio,
            progress_pct: 42,
        };
        let mobile: MobileProximityVerifierEvent = event.into();
        match mobile {
            MobileProximityVerifierEvent::InProgress {
                method,
                progress_pct,
            } => {
                assert_eq!(method, MobileVerifierMethod::AmbientAudio);
                assert_eq!(progress_pct, 42);
            }
            other => panic!("Expected InProgress, got {:?}", other),
        }
    }

    #[test]
    fn event_method_failed_converts() {
        let event = ProximityVerifierEvent::MethodFailed {
            method: VerifierMethod::Ultrasonic,
            reason: "No response".to_string(),
        };
        let mobile: MobileProximityVerifierEvent = event.into();
        match mobile {
            MobileProximityVerifierEvent::MethodFailed { method, reason } => {
                assert_eq!(method, MobileVerifierMethod::Ultrasonic);
                assert_eq!(reason, "No response");
            }
            other => panic!("Expected MethodFailed, got {:?}", other),
        }
    }

    #[test]
    fn event_falling_back_converts() {
        let event = ProximityVerifierEvent::FallingBack {
            failed_method: VerifierMethod::Ultrasonic,
            next_method: VerifierMethod::AmbientAudio,
        };
        let mobile: MobileProximityVerifierEvent = event.into();
        match mobile {
            MobileProximityVerifierEvent::FallingBack {
                failed_method,
                next_method,
            } => {
                assert_eq!(failed_method, MobileVerifierMethod::Ultrasonic);
                assert_eq!(next_method, MobileVerifierMethod::AmbientAudio);
            }
            other => panic!("Expected FallingBack, got {:?}", other),
        }
    }

    #[test]
    fn event_completed_converts() {
        let event = ProximityVerifierEvent::Completed {
            method: VerifierMethod::AmbientAudio,
            confidence: ProximityConfidence::High,
        };
        let mobile: MobileProximityVerifierEvent = event.into();
        match mobile {
            MobileProximityVerifierEvent::Completed { method, confidence } => {
                assert_eq!(method, MobileVerifierMethod::AmbientAudio);
                assert_eq!(confidence, MobileProximityConfidence::High);
            }
            other => panic!("Expected Completed, got {:?}", other),
        }
    }

    #[test]
    fn event_all_methods_exhausted_converts() {
        let event = ProximityVerifierEvent::AllMethodsExhausted;
        let mobile: MobileProximityVerifierEvent = event.into();
        assert!(matches!(
            mobile,
            MobileProximityVerifierEvent::AllMethodsExhausted
        ));
    }
}
