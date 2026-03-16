// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport orchestrator with fallback chain.
//!
//! [`TransportChain`] tries transports in priority order and falls back
//! to the next available transport on failure.
//!
//! **Deprecated (ADR-031):** Fallback is now in `ExchangeSession` via `DeviceCapabilities`.

use super::channel::{TransportChannel, TransportError, TransportType};

/// Policy for what happens when falling back to a different transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackPolicy {
    /// Keep the existing session state across transport switch.
    PreserveSession,
    /// Restart the handshake on the new transport.
    RestartHandshake,
}

/// Ordered chain of transports with automatic fallback.
///
/// **Deprecated (ADR-031):** Transport fallback is now handled inside
/// `ExchangeSession` via `DeviceCapabilities` and `HardwareUnavailable` events.
#[deprecated(note = "ADR-031: transport fallback now in ExchangeSession via DeviceCapabilities")]
pub struct TransportChain {
    transports: Vec<Box<dyn TransportChannel>>,
    policy: FallbackPolicy,
}

impl TransportChain {
    /// Create a new transport chain with the given transports and fallback policy.
    pub fn new(transports: Vec<Box<dyn TransportChannel>>, policy: FallbackPolicy) -> Self {
        Self { transports, policy }
    }

    /// Return the configured fallback policy.
    pub fn policy(&self) -> FallbackPolicy {
        self.policy
    }

    /// Select the first available transport in the chain.
    ///
    /// Returns `TransportError::NoCommonTransport` if none are available.
    pub fn select_transport(&self) -> Result<&dyn TransportChannel, TransportError> {
        for transport in &self.transports {
            if transport.is_available()? {
                return Ok(transport.as_ref());
            }
        }
        Err(TransportError::NoCommonTransport)
    }

    /// Send data, falling back through the chain on failure.
    ///
    /// Tries each available transport in order. If `send` fails on one,
    /// moves to the next. Returns a reference to the transport that
    /// successfully sent the data.
    pub fn send_with_fallback(&self, data: &[u8]) -> Result<&dyn TransportChannel, TransportError> {
        let mut last_err = None;

        for transport in &self.transports {
            if !transport.is_available().unwrap_or(false) {
                continue;
            }
            match transport.send(data) {
                Ok(()) => return Ok(transport.as_ref()),
                Err(e) => {
                    last_err = Some(e);
                    continue;
                }
            }
        }

        Err(last_err.unwrap_or(TransportError::NoCommonTransport))
    }

    /// List transport types that are currently available.
    pub fn available_transports(&self) -> Vec<TransportType> {
        self.transports
            .iter()
            .filter(|t| t.is_available().unwrap_or(false))
            .map(|t| t.transport_type())
            .collect()
    }
}
