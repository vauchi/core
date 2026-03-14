// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Verifier Chain Orchestrator
//!
//! Tries proximity verifiers in priority order. Each verifier gets one
//! attempt. On failure, the chain emits a FallingBack event and moves
//! to the next verifier. The first successful verifier determines the
//! final confidence level.

use std::sync::Mutex;
use std::time::Duration;

use super::verifier_event::{ProximityVerifierEvent, VerifierEventLog, VerifierMethod};
use super::{ProximityConfidence, ProximityError, ProximityVerifier};

/// Entry in the verifier chain: a method label + the verifier itself.
struct ChainEntry {
    method: VerifierMethod,
    verifier: Box<dyn ProximityVerifier>,
}

/// Priority-ordered chain of proximity verifiers.
///
/// Verifiers are tried in the order they were added. The chain stops
/// at the first successful verification and reports its confidence.
///
/// Implements `ProximityVerifier` so it can be used directly as the
/// verifier for `ExchangeSession` (no generic type parameter needed).
///
/// **Nesting unsupported**: Do not add a `VerifierChain` as an entry
/// inside another `VerifierChain`. The inner chain's event log would
/// be silently discarded.
pub struct VerifierChain {
    entries: Vec<ChainEntry>,
    /// Event log from the most recent `verify_proximity_two_way` call.
    /// Stored behind Mutex because `ProximityVerifier` methods take `&self`.
    last_event_log: Mutex<Option<VerifierEventLog>>,
}

impl VerifierChain {
    pub fn new() -> Self {
        VerifierChain {
            entries: Vec::new(),
            last_event_log: Mutex::new(None),
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

impl VerifierChain {
    /// Returns a clone of the event log from the most recent verification attempt.
    ///
    /// Returns `None` if `verify_proximity_two_way` (via the `ProximityVerifier`
    /// trait) has not been called yet.
    pub fn last_event_log(&self) -> Option<VerifierEventLog> {
        self.last_event_log.lock().expect("mutex poisoned").clone()
    }
}

/// Implements `ProximityVerifier` so that `VerifierChain` can be stored
/// directly in `ExchangeSession` as `Box<dyn ProximityVerifier>`.
///
/// **Safety-net no-ops (C1)**: The individual methods (`emit_challenge`,
/// `listen_for_response`, `verify_response`) return `Err(NotSupported)`
/// because the chain operates at the `verify_proximity_two_way` level,
/// not at the individual challenge level. If the `verify_proximity_two_way`
/// override were accidentally removed, the default trait implementation
/// would call these methods and get `Err(NotSupported)` — failing safely
/// instead of silently passing verification.
impl ProximityVerifier for VerifierChain {
    fn confidence_level(&self) -> ProximityConfidence {
        // Return the highest confidence level among entries, or Unknown if empty.
        self.entries
            .iter()
            .map(|e| e.verifier.confidence_level())
            .max_by_key(|c| match c {
                ProximityConfidence::High => 3,
                ProximityConfidence::Medium => 2,
                ProximityConfidence::Low => 1,
                ProximityConfidence::Unknown => 0,
            })
            .unwrap_or(ProximityConfidence::Unknown)
    }

    fn emit_challenge(&self, _challenge: &[u8; 16]) -> Result<(), ProximityError> {
        // Safety-net: chain verification is at the two_way level, not individual.
        Err(ProximityError::NotSupported)
    }

    fn listen_for_response(&self, _timeout: Duration) -> Result<Vec<u8>, ProximityError> {
        // Safety-net: chain verification is at the two_way level, not individual.
        Err(ProximityError::NotSupported)
    }

    fn verify_response(&self, _challenge: &[u8; 16], _response: &[u8]) -> bool {
        // Safety-net: chain verification is at the two_way level, not individual.
        false
    }

    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> Result<(), ProximityError> {
        let log = self.verify(emit_challenge, listen_challenge, timeout, is_initiator);
        let succeeded = log.is_completed();

        // Store the event log for later retrieval via last_event_log()
        *self.last_event_log.lock().expect("mutex poisoned") = Some(log);

        if succeeded {
            Ok(())
        } else {
            Err(ProximityError::NoResponse)
        }
    }
}
