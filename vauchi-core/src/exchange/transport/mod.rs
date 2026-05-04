// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport types and utilities for exchange protocols.
//!
//! Transport negotiation, capabilities, animated QR, and tracing.
//! Hardware I/O uses the `Command`/`Event` protocol (ADR-031).

pub mod animated_qr;
pub mod caps;
pub mod negotiation;
pub mod protocol;
pub mod trace;

use serde::Serialize;
use std::fmt;

pub use caps::TransportCaps;
pub use negotiation::negotiate_transport;

/// Identifies a transport mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TransportType {
    WifiAware,
    Ble,
    AnimatedQr,
    StaticQr,
    Nfc,
    Tcp,
}

impl TransportType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WifiAware => "wifi_aware",
            Self::Ble => "ble",
            Self::AnimatedQr => "animated_qr",
            Self::StaticQr => "static_qr",
            Self::Nfc => "nfc",
            Self::Tcp => "tcp",
        }
    }

    /// Priority for auto-negotiation. Higher = preferred.
    pub fn priority(&self) -> u8 {
        match self {
            Self::WifiAware => 50,
            Self::Tcp => 45,
            Self::Nfc => 42,
            Self::Ble => 40,
            Self::AnimatedQr => 30,
            Self::StaticQr => 20,
        }
    }
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
