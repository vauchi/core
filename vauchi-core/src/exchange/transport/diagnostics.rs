// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport diagnostics — probe and report transport availability.
//!
//! [`TransportDiagnostics`] wraps a set of transports and provides
//! methods to check which ones are currently available on the device.

use super::channel::{TransportChannel, TransportType};

/// Result of probing a single transport for availability.
#[derive(Debug)]
pub struct TransportProbeResult {
    /// Which transport was probed.
    pub transport: TransportType,
    /// Whether the transport reported itself as available.
    pub available: bool,
    /// Error message if `is_available()` returned `Err`.
    pub error: Option<String>,
}

/// Probes and reports on transport availability.
///
/// Wraps a collection of [`TransportChannel`] implementations and
/// provides convenience methods to check which transports are usable.
pub struct TransportDiagnostics {
    transports: Vec<Box<dyn TransportChannel>>,
}

impl TransportDiagnostics {
    /// Create a new diagnostics instance wrapping the given transports.
    pub fn new(transports: Vec<Box<dyn TransportChannel>>) -> Self {
        Self { transports }
    }

    /// Probe all transports and return their availability status.
    pub fn probe_all(&self) -> Vec<TransportProbeResult> {
        self.transports
            .iter()
            .map(|t| {
                let transport = t.transport_type();
                match t.is_available() {
                    Ok(available) => TransportProbeResult {
                        transport,
                        available,
                        error: None,
                    },
                    Err(e) => TransportProbeResult {
                        transport,
                        available: false,
                        error: Some(e.to_string()),
                    },
                }
            })
            .collect()
    }

    /// Probe a specific transport type. Returns `None` if no transport
    /// of the given type is registered.
    pub fn probe(&self, transport_type: TransportType) -> Option<TransportProbeResult> {
        self.transports
            .iter()
            .find(|t| t.transport_type() == transport_type)
            .map(|t| match t.is_available() {
                Ok(available) => TransportProbeResult {
                    transport: transport_type,
                    available,
                    error: None,
                },
                Err(e) => TransportProbeResult {
                    transport: transport_type,
                    available: false,
                    error: Some(e.to_string()),
                },
            })
    }

    /// List all transport types that report as available.
    pub fn available_types(&self) -> Vec<TransportType> {
        self.probe_all()
            .into_iter()
            .filter(|r| r.available)
            .map(|r| r.transport)
            .collect()
    }
}
