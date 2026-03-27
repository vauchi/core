// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structured event log for exchange diagnostics.
//!
//! [`TraceLog`] records timestamped [`TraceEventKind`] entries during an
//! exchange session and can produce a [`TraceSummary`] or JSON export for
//! post-session analysis.

use super::channel::TransportType;
use serde::Serialize;
use std::time::Instant;

/// A single timestamped trace event.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    /// Microseconds since the trace log was created.
    pub timestamp_us: u64,
    /// The event that occurred.
    pub event: TraceEventKind,
}

/// The kind of trace event recorded during an exchange.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceEventKind {
    PeerDiscovered {
        peer_id: String,
    },
    PeerLost {
        peer_id: String,
    },
    CapabilitiesExchanged {
        ours: u16,
        theirs: u16,
    },
    TransportSelected {
        selected: TransportType,
    },
    FallbackTriggered {
        from: TransportType,
        to: TransportType,
        reason: String,
    },
    HandshakeStarted,
    KeyOfferSent {
        size: usize,
    },
    KeyOfferReceived {
        size: usize,
    },
    SharedKeyDerived,
    CardEncrypted {
        size: usize,
    },
    CardDecrypted {
        size: usize,
    },
    ExchangeComplete,
    TransportError {
        transport: TransportType,
        error: String,
    },
    QrFrameDisplayed {
        index: usize,
        total: usize,
    },
    QrFrameScanned {
        index: usize,
        total: usize,
    },
    QrProgress {
        received: usize,
        total: usize,
    },
    WifiAwarePublishing,
    WifiAwareSubscribing,
    WifiAwareConnected,
}

/// Summary statistics extracted from a completed trace log.
#[derive(Debug, Clone, Serialize)]
pub struct TraceSummary {
    /// The transport ultimately used for the exchange, if any.
    pub transport_used: Option<TransportType>,
    /// Each fallback that occurred: (from, to).
    pub fallbacks: Vec<(TransportType, TransportType)>,
    /// Total duration from first to last event, in microseconds.
    pub total_duration_us: u64,
    /// Total bytes transferred (key offers + card encrypt/decrypt).
    pub bytes_transferred: usize,
}

/// Append-only log of trace events for a single exchange session.
pub struct TraceLog {
    start: Instant,
    events: Vec<TraceEvent>,
}

impl TraceLog {
    /// Create a new empty trace log. The internal clock starts now.
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            events: Vec::new(),
        }
    }

    /// Record a trace event with the current timestamp.
    pub fn record(&mut self, event: TraceEventKind) {
        let timestamp_us = self.start.elapsed().as_micros() as u64;
        self.events.push(TraceEvent {
            timestamp_us,
            event,
        });
    }

    /// Return a slice of all recorded events.
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    /// Serialize the event log to a JSON string.
    pub fn export_json(&self) -> String {
        serde_json::to_string_pretty(&self.events).unwrap_or_else(|_| "[]".to_string())
    }

    /// Compute a summary from the recorded events.
    pub fn summary(&self) -> TraceSummary {
        let mut transport_used = None;
        let mut fallbacks = Vec::new();
        let mut bytes_transferred: usize = 0;

        for event in &self.events {
            match &event.event {
                TraceEventKind::TransportSelected { selected } => {
                    transport_used = Some(*selected);
                }
                TraceEventKind::FallbackTriggered { from, to, .. } => {
                    fallbacks.push((*from, *to));
                }
                TraceEventKind::KeyOfferSent { size }
                | TraceEventKind::KeyOfferReceived { size }
                | TraceEventKind::CardEncrypted { size }
                | TraceEventKind::CardDecrypted { size } => {
                    bytes_transferred += size;
                }
                _ => {}
            }
        }

        let total_duration_us = match (self.events.first(), self.events.last()) {
            (Some(first), Some(last)) if self.events.len() >= 2 => {
                last.timestamp_us - first.timestamp_us
            }
            _ => 0,
        };

        TraceSummary {
            transport_used,
            fallbacks,
            total_duration_us,
            bytes_transferred,
        }
    }
}

impl Default for TraceLog {
    fn default() -> Self {
        Self::new()
    }
}
