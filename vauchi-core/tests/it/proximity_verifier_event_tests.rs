// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for ProximityVerifierEvent — observable event system for
//! proximity verification progress.
//!
//! Events allow the platform/UI layer to show meaningful status:
//! "Listening for ambient audio...", "Tap detected, verifying...",
//! "Audio failed, trying accelerometer...", "Verified!"

#![cfg(feature = "testing")]

use vauchi_core::exchange::ProximityConfidence;
use vauchi_core::exchange::verifier_event::{
    ProximityVerifierEvent, VerifierEventLog, VerifierMethod,
};

// ===== VerifierMethod =====

// @internal
#[test]
fn verifier_method_display() {
    assert_eq!(VerifierMethod::Ultrasonic.as_str(), "ultrasonic");
    assert_eq!(VerifierMethod::AmbientAudio.as_str(), "ambient_audio");
    assert_eq!(VerifierMethod::Accelerometer.as_str(), "accelerometer");
    assert_eq!(
        VerifierMethod::ManualConfirmation.as_str(),
        "manual_confirmation"
    );
    assert_eq!(VerifierMethod::Nfc.as_str(), "nfc");
    assert_eq!(VerifierMethod::Ble.as_str(), "ble");
}

// ===== ProximityVerifierEvent variants =====

// @internal
#[test]
fn waiting_for_action_event() {
    let event = ProximityVerifierEvent::WaitingForAction {
        method: VerifierMethod::Accelerometer,
        instruction: "Tap the table with both phones".into(),
    };
    assert!(matches!(
        event,
        ProximityVerifierEvent::WaitingForAction {
            method: VerifierMethod::Accelerometer,
            ..
        }
    ));
}

// @internal
#[test]
fn in_progress_event() {
    let event = ProximityVerifierEvent::InProgress {
        method: VerifierMethod::AmbientAudio,
        progress_pct: 50,
    };
    if let ProximityVerifierEvent::InProgress { progress_pct, .. } = event {
        assert_eq!(progress_pct, 50);
    } else {
        panic!("Expected InProgress event");
    }
}

// @internal
#[test]
fn method_failed_event() {
    let event = ProximityVerifierEvent::MethodFailed {
        method: VerifierMethod::Ultrasonic,
        reason: "No microphone access".into(),
    };
    if let ProximityVerifierEvent::MethodFailed { method, reason } = event {
        assert_eq!(method, VerifierMethod::Ultrasonic);
        assert!(reason.contains("microphone"));
    } else {
        panic!("Expected MethodFailed event");
    }
}

// @internal
#[test]
fn falling_back_event() {
    let event = ProximityVerifierEvent::FallingBack {
        failed_method: VerifierMethod::Ultrasonic,
        next_method: VerifierMethod::AmbientAudio,
    };
    if let ProximityVerifierEvent::FallingBack {
        failed_method,
        next_method,
    } = event
    {
        assert_eq!(failed_method, VerifierMethod::Ultrasonic);
        assert_eq!(next_method, VerifierMethod::AmbientAudio);
    } else {
        panic!("Expected FallingBack event");
    }
}

// @internal
#[test]
fn completed_event() {
    let event = ProximityVerifierEvent::Completed {
        method: VerifierMethod::AmbientAudio,
        confidence: ProximityConfidence::High,
    };
    if let ProximityVerifierEvent::Completed { method, confidence } = event {
        assert_eq!(method, VerifierMethod::AmbientAudio);
        assert_eq!(confidence, ProximityConfidence::High);
    } else {
        panic!("Expected Completed event");
    }
}

// @internal
#[test]
fn all_methods_exhausted_event() {
    let event = ProximityVerifierEvent::AllMethodsExhausted;
    assert!(matches!(event, ProximityVerifierEvent::AllMethodsExhausted));
}

// ===== VerifierEventLog =====

// @internal
#[test]
fn event_log_starts_empty() {
    let log = VerifierEventLog::new();
    assert!(log.events().is_empty());
}

// @internal
#[test]
fn event_log_records_events() {
    let mut log = VerifierEventLog::new();
    log.push(ProximityVerifierEvent::InProgress {
        method: VerifierMethod::Ultrasonic,
        progress_pct: 25,
    });
    log.push(ProximityVerifierEvent::MethodFailed {
        method: VerifierMethod::Ultrasonic,
        reason: "timeout".into(),
    });
    assert_eq!(log.events().len(), 2);
}

// @internal
#[test]
fn event_log_last_event() {
    let mut log = VerifierEventLog::new();
    log.push(ProximityVerifierEvent::InProgress {
        method: VerifierMethod::AmbientAudio,
        progress_pct: 100,
    });
    log.push(ProximityVerifierEvent::Completed {
        method: VerifierMethod::AmbientAudio,
        confidence: ProximityConfidence::High,
    });

    let last = log.last().unwrap();
    assert!(matches!(last, ProximityVerifierEvent::Completed { .. }));
}

// @internal
#[test]
fn event_log_is_completed() {
    let mut log = VerifierEventLog::new();
    assert!(!log.is_completed());

    log.push(ProximityVerifierEvent::Completed {
        method: VerifierMethod::AmbientAudio,
        confidence: ProximityConfidence::High,
    });
    assert!(log.is_completed());
}

// @internal
#[test]
fn event_log_is_exhausted() {
    let mut log = VerifierEventLog::new();
    assert!(!log.is_exhausted());

    log.push(ProximityVerifierEvent::AllMethodsExhausted);
    assert!(log.is_exhausted());
}

// @internal
#[test]
fn event_log_final_confidence() {
    let mut log = VerifierEventLog::new();
    assert_eq!(log.final_confidence(), None);

    log.push(ProximityVerifierEvent::Completed {
        method: VerifierMethod::Accelerometer,
        confidence: ProximityConfidence::High,
    });
    assert_eq!(log.final_confidence(), Some(ProximityConfidence::High));
}

// @internal
#[test]
fn event_log_final_confidence_none_when_exhausted() {
    let mut log = VerifierEventLog::new();
    log.push(ProximityVerifierEvent::AllMethodsExhausted);
    assert_eq!(log.final_confidence(), None);
}
