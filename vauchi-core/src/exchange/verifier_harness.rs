// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-Device Proximity Verification Test Harness
//!
//! Provides `SimulatedPeer`, `Scenario`, and `VerificationOutcome` for
//! testing the VerifierChain across device configurations and conditions.

use super::proximity::ProximityVerifier;
use super::verifier_chain::VerifierChain;
use super::verifier_event::{ProximityVerifierEvent, VerifierMethod};
use super::{ManualConfirmationVerifier, MockProximityVerifier, ProximityConfidence};
use std::collections::HashSet;
use std::time::Duration;

/// Capabilities of a simulated device.
#[derive(Debug, Clone)]
pub struct PeerCapabilities {
    pub ultrasonic: bool,
    pub ambient_audio: bool,
    pub accelerometer: bool,
    pub manual_confirmation: bool,
}

impl PeerCapabilities {
    /// Methods available on this peer.
    pub fn available_methods(&self) -> Vec<VerifierMethod> {
        let mut methods = Vec::new();
        if self.ultrasonic {
            methods.push(VerifierMethod::Ultrasonic);
        }
        if self.ambient_audio {
            methods.push(VerifierMethod::AmbientAudio);
        }
        if self.accelerometer {
            methods.push(VerifierMethod::Accelerometer);
        }
        if self.manual_confirmation {
            methods.push(VerifierMethod::ManualConfirmation);
        }
        methods
    }
}

/// A simulated device with configurable capabilities.
#[derive(Debug, Clone)]
pub struct SimulatedPeer {
    caps: PeerCapabilities,
}

impl SimulatedPeer {
    pub fn new(caps: PeerCapabilities) -> Self {
        SimulatedPeer { caps }
    }

    /// Typical mobile device: all methods available.
    pub fn mobile() -> Self {
        SimulatedPeer::new(PeerCapabilities {
            ultrasonic: true,
            ambient_audio: true,
            accelerometer: true,
            manual_confirmation: true,
        })
    }

    /// Typical desktop: ambient audio + manual confirmation.
    /// No ultrasonic (requires specialized speaker/mic at 18+ kHz).
    /// No accelerometer (desktops don't have one).
    pub fn desktop() -> Self {
        SimulatedPeer::new(PeerCapabilities {
            ultrasonic: false,
            ambient_audio: true,
            accelerometer: false,
            manual_confirmation: true,
        })
    }

    pub fn capabilities(&self) -> &PeerCapabilities {
        &self.caps
    }
}

/// Result of running a verification scenario.
#[derive(Debug)]
pub struct VerificationOutcome {
    pub confidence: Option<ProximityConfidence>,
    pub method_used: Option<VerifierMethod>,
    pub events: Vec<ProximityVerifierEvent>,
}

impl VerificationOutcome {
    pub fn is_success(&self) -> bool {
        self.confidence.is_some()
    }
}

/// A test scenario: two peers + environment conditions.
pub struct Scenario {
    peer_a: SimulatedPeer,
    peer_b: SimulatedPeer,
    co_located: bool,
    /// Methods that should be forced to fail (overrides co_located).
    failing_methods: HashSet<VerifierMethod>,
    /// Force all verifiers to succeed (regardless of co_located).
    force_all_succeed: bool,
    /// Force all verifiers to time out.
    force_all_timeout: bool,
}

impl Scenario {
    pub fn new(peer_a: SimulatedPeer, peer_b: SimulatedPeer, co_located: bool) -> Self {
        Scenario {
            peer_a,
            peer_b,
            co_located,
            failing_methods: HashSet::new(),
            force_all_succeed: false,
            force_all_timeout: false,
        }
    }

    pub fn with_verifier_failing(mut self, method: VerifierMethod) -> Self {
        self.failing_methods.insert(method);
        self
    }

    pub fn with_all_verifiers_succeeding(mut self) -> Self {
        self.force_all_succeed = true;
        self
    }

    pub fn with_all_verifiers_timing_out(mut self) -> Self {
        self.force_all_timeout = true;
        self
    }

    /// Run the scenario and return the outcome.
    ///
    /// The harness computes the intersection of both peers' capabilities,
    /// builds a VerifierChain with mock verifiers configured per the
    /// scenario conditions, and runs it.
    pub fn run(&self) -> VerificationOutcome {
        // Find common methods (intersection of both peers' capabilities)
        let a_methods: HashSet<_> = self.peer_a.caps.available_methods().into_iter().collect();
        let b_methods: HashSet<_> = self.peer_b.caps.available_methods().into_iter().collect();
        let common: Vec<_> = priority_ordered_methods()
            .into_iter()
            .filter(|m| a_methods.contains(m) && b_methods.contains(m))
            .collect();

        if common.is_empty() {
            return VerificationOutcome {
                confidence: None,
                method_used: None,
                events: vec![ProximityVerifierEvent::AllMethodsExhausted],
            };
        }

        // Build chain with mock verifiers
        let mut chain = VerifierChain::new();
        for method in &common {
            let verifier: Box<dyn super::ProximityVerifier> = if self.force_all_timeout {
                Box::new(MockProximityVerifier::timeout())
            } else if self.failing_methods.contains(method)
                || (!self.co_located
                    && !self.force_all_succeed
                    && *method != VerifierMethod::ManualConfirmation)
            {
                Box::new(MockProximityVerifier::failure())
            } else if self.force_all_succeed
                || self.co_located
                || *method == VerifierMethod::ManualConfirmation
            {
                if *method == VerifierMethod::ManualConfirmation {
                    let mv = ManualConfirmationVerifier::new();
                    mv.confirm();
                    Box::new(mv)
                } else {
                    Box::new(MockProximityVerifier::success())
                }
            } else {
                Box::new(MockProximityVerifier::failure())
            };
            chain.add(*method, verifier);
        }

        let emit_challenge = [0x0Au8; 16];
        let listen_challenge = [0x0Bu8; 16];
        let timeout = Duration::from_secs(5);
        // Use asymmetric challenges to exercise the two-way protocol path.
        let _ = chain.verify_proximity_two_way(&emit_challenge, &listen_challenge, timeout, true);
        let log = chain
            .last_event_log()
            .expect("log should exist after verification");

        let confidence = log.final_confidence();
        let method_used = log.events().iter().find_map(|e| match e {
            ProximityVerifierEvent::Completed { method, .. } => Some(*method),
            _ => None,
        });

        VerificationOutcome {
            confidence,
            method_used,
            events: log.events().to_vec(),
        }
    }
}

/// Methods in priority order (highest first).
fn priority_ordered_methods() -> Vec<VerifierMethod> {
    vec![
        VerifierMethod::Ultrasonic,
        VerifierMethod::AmbientAudio,
        VerifierMethod::Accelerometer,
        VerifierMethod::ManualConfirmation,
    ]
}
