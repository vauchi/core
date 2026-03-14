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

/// Snapshot of the most recent `verify_proximity_two_way` result.
///
/// Stored in a single Mutex to ensure log and confidence are always
/// written and read atomically.
struct VerificationResult {
    log: VerifierEventLog,
    winning_confidence: Option<ProximityConfidence>,
}

/// Callback invoked for each event during verification.
///
/// Receives a reference to each `ProximityVerifierEvent` as it occurs,
/// enabling real-time UI updates (progress bars, fallback indicators).
type EventCallback = Box<dyn Fn(&ProximityVerifierEvent) + Send + Sync>;

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
    /// Result from the most recent `verify_proximity_two_way` call.
    /// Single Mutex ensures log + confidence are atomically consistent.
    last_result: Mutex<Option<VerificationResult>>,
    /// Optional callback for real-time event emission during verification.
    event_callback: Option<EventCallback>,
}

impl VerifierChain {
    pub fn new() -> Self {
        VerifierChain {
            entries: Vec::new(),
            last_result: Mutex::new(None),
            event_callback: None,
        }
    }

    /// Add a verifier to the end of the chain (lowest priority so far).
    pub fn add(&mut self, method: VerifierMethod, verifier: Box<dyn ProximityVerifier>) {
        self.entries.push(ChainEntry { method, verifier });
    }

    /// Set a callback for real-time event emission during verification.
    ///
    /// The callback receives each `ProximityVerifierEvent` as it occurs,
    /// before it is stored in the event log. Currently used by integration
    /// tests; will be wired into the platform layer for live UI progress.
    #[cfg(any(test, feature = "testing"))]
    pub fn set_event_callback(
        &mut self,
        callback: impl Fn(&ProximityVerifierEvent) + Send + Sync + 'static,
    ) {
        self.event_callback = Some(Box::new(callback));
    }

    /// Push an event to the log and fire the callback (if set).
    fn emit(&self, log: &mut VerifierEventLog, event: ProximityVerifierEvent) {
        if let Some(ref cb) = self.event_callback {
            cb(&event);
        }
        log.push(event);
    }

    /// Run the verification chain. Always returns a log — never fails.
    ///
    /// Tries each verifier in order using `verify_proximity_two_way`.
    /// Emits events for each attempt. Returns a log of all events.
    /// Called only from `verify_proximity_two_way`; not part of the public API.
    fn verify(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> VerifierEventLog {
        let mut log = VerifierEventLog::new();

        if self.entries.is_empty() {
            self.emit(&mut log, ProximityVerifierEvent::AllMethodsExhausted);
            return log;
        }

        for (idx, entry) in self.entries.iter().enumerate() {
            self.emit(
                &mut log,
                ProximityVerifierEvent::InProgress {
                    method: entry.method,
                    progress_pct: 0,
                },
            );

            let result = entry.verifier.verify_proximity_two_way(
                emit_challenge,
                listen_challenge,
                timeout,
                is_initiator,
            );

            match result {
                Ok(()) => {
                    let confidence = entry.verifier.confidence_level();
                    self.emit(
                        &mut log,
                        ProximityVerifierEvent::Completed {
                            method: entry.method,
                            confidence,
                        },
                    );
                    return log;
                }
                Err(err) => {
                    self.emit(
                        &mut log,
                        ProximityVerifierEvent::MethodFailed {
                            method: entry.method,
                            reason: err.to_string(),
                        },
                    );

                    if let Some(next) = self.entries.get(idx + 1) {
                        self.emit(
                            &mut log,
                            ProximityVerifierEvent::FallingBack {
                                failed_method: entry.method,
                                next_method: next.method,
                            },
                        );
                    }
                }
            }
        }

        self.emit(&mut log, ProximityVerifierEvent::AllMethodsExhausted);
        log
    }

    /// Returns a clone of the event log from the most recent verification attempt.
    ///
    /// Returns `None` if `verify_proximity_two_way` (via the `ProximityVerifier`
    /// trait) has not been called yet.
    pub fn last_event_log(&self) -> Option<VerifierEventLog> {
        self.last_result
            .lock()
            .expect("mutex poisoned")
            .as_ref()
            .map(|r| r.log.clone())
    }
}

impl Default for VerifierChain {
    fn default() -> Self {
        Self::new()
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
        // Return the winning verifier's confidence from the last successful run.
        // Falls back to Unknown if no verification has been performed yet.
        self.last_result
            .lock()
            .expect("mutex poisoned")
            .as_ref()
            .and_then(|r| r.winning_confidence)
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
        // Returns false so the default verify_proximity impl fails safely
        // if this method is ever reached unexpectedly.
        false
    }

    fn verification_event_log(&self) -> Option<super::verifier_event::VerifierEventLog> {
        self.last_event_log()
    }

    fn verify_proximity_two_way(
        &self,
        emit_challenge: &[u8; 16],
        listen_challenge: &[u8; 16],
        timeout: Duration,
        is_initiator: bool,
    ) -> Result<(), ProximityError> {
        let log = self.verify(emit_challenge, listen_challenge, timeout, is_initiator);
        let winning_confidence = log.final_confidence();

        // Store log + confidence atomically in a single lock acquisition
        *self.last_result.lock().expect("mutex poisoned") = Some(VerificationResult {
            log,
            winning_confidence,
        });

        if winning_confidence.is_some() {
            Ok(())
        } else {
            Err(ProximityError::NoResponse)
        }
    }
}
