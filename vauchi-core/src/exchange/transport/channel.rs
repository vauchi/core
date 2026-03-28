// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Core transport channel trait and types.
//!
//! All exchange transports (BLE, WiFi Aware, Animated QR, etc.) implement
//! [`TransportChannel`] to provide raw bidirectional byte exchange.

use super::caps::TransportCaps;
use serde::Serialize;
use std::fmt;
use std::time::Duration;
use thiserror::Error;

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
            Self::Ble => 40,
            Self::AnimatedQr => 30,
            Self::StaticQr => 20,
            Self::Nfc => 42, // Above BLE (40), below TCP (45): physical tap > RSSI heuristic
        }
    }
}

impl fmt::Display for TransportType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Information about a discovered peer.
#[derive(Debug, Clone)]
pub struct PeerInfo {
    pub peer_id: String,
    pub capabilities: TransportCaps,
    pub rssi: Option<i8>,
}

/// Transport-level errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum TransportError {
    #[error("transport {transport} unavailable: {reason}")]
    Unavailable {
        transport: TransportType,
        reason: String,
    },
    #[error("transport {transport} timed out after {timeout_ms}ms")]
    Timeout {
        transport: TransportType,
        timeout_ms: u64,
    },
    #[error("transport {transport} connection lost: {reason}")]
    ConnectionLost {
        transport: TransportType,
        reason: String,
    },
    #[error("transport {transport} send failed: {reason}")]
    SendFailed {
        transport: TransportType,
        reason: String,
    },
    #[error("transport {transport} receive failed: {reason}")]
    ReceiveFailed {
        transport: TransportType,
        reason: String,
    },
    #[error("no common transport with peer")]
    NoCommonTransport,
    #[error("payload too large: {size} > {max}")]
    PayloadTooLarge { size: usize, max: usize },
}

/// Raw bidirectional byte channel. All transports implement this trait.
///
/// **Deprecated (ADR-031):** Use `ExchangeCommand`/`ExchangeHardwareEvent` instead.
/// Core no longer owns hardware I/O — frontends execute commands and report events.
/// This trait will be removed in a future version.
#[deprecated(note = "ADR-031: use ExchangeCommand/ExchangeHardwareEvent instead")]
pub trait TransportChannel: Send + Sync {
    /// Which transport this channel uses.
    fn transport_type(&self) -> TransportType;

    /// Check if this transport is available on the current device.
    fn is_available(&self) -> Result<bool, TransportError>;

    /// Discover a nearby peer. Blocks up to `timeout`.
    fn discover_peer(&self, timeout: Duration) -> Result<PeerInfo, TransportError>;

    /// Send data to the connected peer.
    fn send(&self, data: &[u8]) -> Result<(), TransportError>;

    /// Receive data from the connected peer. Blocks up to `timeout`.
    fn receive(&self, timeout: Duration) -> Result<Vec<u8>, TransportError>;

    /// Close the transport channel.
    fn close(&self) -> Result<(), TransportError>;

    /// Maximum single-send payload size in bytes.
    fn max_payload_size(&self) -> usize;

    /// Whether payloads larger than `max_payload_size` need chunking.
    fn requires_chunking(&self) -> bool;
}
