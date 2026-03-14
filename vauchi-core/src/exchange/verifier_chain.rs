// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Verifier Chain Orchestrator
//!
//! Tries proximity verifiers in priority order. Each verifier gets one
//! attempt. On failure, the chain emits a FallingBack event and moves
//! to the next verifier. The first successful verifier determines the
//! final confidence level.

use super::verifier_event::{ProximityVerifierEvent, VerifierEventLog, VerifierMethod};
use super::ProximityVerifier;
use std::time::Duration;

/// Entry in the verifier chain: a method label + the verifier itself.
struct ChainEntry {
    method: VerifierMethod,
    verifier: Box<dyn ProximityVerifier>,
}

/// Priority-ordered chain of proximity verifiers.
///
/// Verifiers are tried in the order they were added. The chain stops
/// at the first successful verification and reports its confidence.
pub struct VerifierChain {
    entries: Vec<ChainEntry>,
}

impl VerifierChain {
    pub fn new() -> Self {
        VerifierChain {
            entries: Vec::new(),
        }
    }

    /// Add a verifier to the end of the chain (lowest priority so far).
    pub fn add(&mut self, method: VerifierMethod, verifier: Box<dyn ProximityVerifier>) {
        self.entries.push(ChainEntry { method, verifier });
    }

    /// Run the verification chain.
    ///
    /// Tries each verifier in order using `verify_proximity_two_way`.
    /// Emits events for each attempt. Returns a log of all events.
    /// Run the verification chain. Always returns a log — never fails.
    pub fn verify(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> VerifierEventLog {
        let mut log = VerifierEventLog::new();

        if self.entries.is_empty() {
            log.push(ProximityVerifierEvent::AllMethodsExhausted);
            return log;
        }

        for (idx, entry) in self.entries.iter().enumerate() {
            // Emit InProgress event
            log.push(ProximityVerifierEvent::InProgress {
                method: entry.method,
                progress_pct: 0,
            });

            // Attempt verification
            let result = entry.verifier.verify_proximity_two_way(
                emit_challenge,
                listen_challenge,
                timeout,
                is_initiator,
            );

            match result {
                Ok(()) => {
                    // Success — record completion and stop
                    let confidence = entry.verifier.confidence_level();
                    log.push(ProximityVerifierEvent::Completed {
                        method: entry.method,
                        confidence,
                    });
                    return log;
                }
                Err(err) => {
                    // Failed — record failure
                    log.push(ProximityVerifierEvent::MethodFailed {
                        method: entry.method,
                        reason: err.to_string(),
                    });

                    // If there's a next verifier, emit FallingBack
                    if let Some(next) = self.entries.get(idx + 1) {
                        log.push(ProximityVerifierEvent::FallingBack {
                            failed_method: entry.method,
                            next_method: next.method,
                        });
                    }
                }
            }
        }

        // All verifiers failed
        log.push(ProximityVerifierEvent::AllMethodsExhausted);
        log
    }
}

impl Default for VerifierChain {
    fn default() -> Self {
        Self::new()
    }
}
