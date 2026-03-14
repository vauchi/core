// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Exchange Debug Log
//!
//! Timestamped event collection for exchange flow diagnostics.
//! Each event records elapsed time since session start, enabling
//! performance analysis and debugging of exchange issues.

use serde::Serialize;
use std::fmt::Write;
use std::time::Instant;

/// Events that occur during a contact exchange flow.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExchangeDebugEvent {
    /// Exchange session started.
    SessionStarted { transport: String },
    /// Our QR code was generated.
    QrGenerated,
    /// We scanned the peer's QR code.
    QrScanned,
    /// Cryptographic key agreement completed.
    KeyAgreementCompleted,
    /// Proximity check started with a specific method.
    ProximityCheckStarted { method: String },
    /// Proximity check completed with a confidence result.
    ProximityCheckCompleted { confidence: String },
    /// Exchange completed successfully.
    ExchangeCompleted,
    /// Exchange failed.
    ExchangeFailed { error: String },
}

/// A timestamped exchange event entry.
#[derive(Debug, Clone, Serialize)]
pub struct TimestampedEvent {
    /// Milliseconds elapsed since the log was created.
    pub elapsed_ms: u64,
    /// The event that occurred.
    pub event: ExchangeDebugEvent,
}

/// Ordered log of timestamped exchange events.
///
/// Created once per exchange session. Events are pushed as the exchange
/// progresses. The log can be exported as JSONL for diagnostic analysis.
#[derive(Debug)]
pub struct ExchangeDebugLog {
    start: Instant,
    events: Vec<TimestampedEvent>,
}

impl ExchangeDebugLog {
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            events: Vec::new(),
        }
    }

    /// Record an event with the current elapsed time.
    pub fn push(&mut self, event: ExchangeDebugEvent) {
        // as_millis() returns u128; truncation safe — overflows after ~585M years.
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        self.events.push(TimestampedEvent { elapsed_ms, event });
    }

    /// Get all recorded events.
    pub fn events(&self) -> &[TimestampedEvent] {
        &self.events
    }

    /// Export the log as a human-readable Markdown report.
    pub fn to_markdown(&self) -> String {
        let count = self.events.len();
        let mut md = String::new();
        writeln!(md, "# Exchange Debug Log\n").unwrap();
        writeln!(md, "**{count} events**\n").unwrap();

        if self.events.is_empty() {
            writeln!(md, "_No events recorded._").unwrap();
            return md;
        }

        writeln!(md, "| Elapsed (ms) | Event |").unwrap();
        writeln!(md, "|---:|---|").unwrap();
        for entry in &self.events {
            let desc = Self::event_description(&entry.event);
            writeln!(md, "| {} | {} |", entry.elapsed_ms, desc).unwrap();
        }
        md
    }

    /// Human-readable description of an exchange event.
    fn event_description(event: &ExchangeDebugEvent) -> String {
        match event {
            ExchangeDebugEvent::SessionStarted { transport } => {
                format!("SessionStarted ({transport})")
            }
            ExchangeDebugEvent::QrGenerated => "QrGenerated".to_string(),
            ExchangeDebugEvent::QrScanned => "QrScanned".to_string(),
            ExchangeDebugEvent::KeyAgreementCompleted => "KeyAgreementCompleted".to_string(),
            ExchangeDebugEvent::ProximityCheckStarted { method } => {
                format!("ProximityCheckStarted ({method})")
            }
            ExchangeDebugEvent::ProximityCheckCompleted { confidence } => {
                format!("ProximityCheckCompleted ({confidence})")
            }
            ExchangeDebugEvent::ExchangeCompleted => "ExchangeCompleted".to_string(),
            ExchangeDebugEvent::ExchangeFailed { error } => {
                format!("ExchangeFailed: {error}")
            }
        }
    }

    /// Export the log as JSONL (one JSON object per line).
    pub fn to_jsonl(&self) -> String {
        self.events
            .iter()
            .map(|e| serde_json::to_string(e).expect("TimestampedEvent serialization cannot fail"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl Default for ExchangeDebugLog {
    fn default() -> Self {
        Self::new()
    }
}
