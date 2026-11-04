// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! TCP-based exchange transport for USB cable and local network exchanges.
//!
//! Provides a simple length-prefixed protocol for exchanging payloads over
//! a TCP stream. Used by platform USB adapters (Android tethering, iOS
//! usbmuxd, desktop USB detection) and potentially local Wi-Fi exchange.
//!
//! Protocol:
//! 1. Both sides send their exchange payload (4-byte big-endian length + data)
//! 2. Both sides receive the peer's payload
//! 3. Payloads are the same format as QR data strings (base64-encoded ExchangeQR)
//!
//! The protocol is symmetric — both sides run the same code. The initiator
//! connects, the responder listens.

use std::io;
use std::net::TcpStream;

use vauchi_contract::direct::{self, DirectFrameError, MAX_DIRECT_PAYLOAD};

use super::direct_transport::ProximityLevel;
use super::error::ExchangeError;

/// Errors from TCP transport operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TcpTransportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid protocol magic")]
    InvalidMagic,

    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u8),

    #[error("Payload too large: {0} bytes (max {MAX_DIRECT_PAYLOAD})")]
    PayloadTooLarge(u32),

    #[error("Connection closed by peer")]
    ConnectionClosed,
}

/// Send an exchange payload over a TCP stream.
///
/// The frame is the contract's (`vauchi_contract::direct`): magic (4) +
/// version (1) + length (4 BE) + payload. Core chooses the framing
/// (ADR-077), and the contract holds the one codec the hardware shells use
/// too (private#1029).
pub fn send_payload(stream: &mut TcpStream, payload: &[u8]) -> Result<(), TcpTransportError> {
    Ok(direct::write_frame(stream, payload)?)
}

/// Receive an exchange payload from a TCP stream.
pub fn recv_payload(stream: &mut TcpStream) -> Result<Vec<u8>, TcpTransportError> {
    Ok(direct::read_frame(stream)?)
}

/// Exchange payloads bidirectionally over a TCP stream.
///
/// The initiator sends first and the responder receives first, so both
/// sides agree on who sends first and neither deadlocks.
pub fn exchange_payloads(
    stream: &mut TcpStream,
    our_payload: &[u8],
    is_initiator: bool,
) -> Result<Vec<u8>, TcpTransportError> {
    Ok(direct::exchange(stream, our_payload, is_initiator)?)
}

impl From<DirectFrameError> for TcpTransportError {
    fn from(error: DirectFrameError) -> Self {
        match error {
            DirectFrameError::Io(error) => Self::Io(error),
            DirectFrameError::InvalidMagic => Self::InvalidMagic,
            DirectFrameError::UnsupportedVersion(version) => Self::UnsupportedVersion(version),
            DirectFrameError::PayloadTooLarge(len) => Self::PayloadTooLarge(len),
            // An empty frame, either way, is what Core always called a closed
            // connection.
            DirectFrameError::EmptyPayload => Self::ConnectionClosed,
            _ => Self::Io(io::Error::other(error.to_string())),
        }
    }
}

/// TCP transport utility for desktop frontends.
///
/// Wraps a `TcpStream` with the VXCH framing protocol. Used by platform
/// frontends to execute [`Command::DirectSend`] — core never
/// calls this directly (ADR-031).
///
/// USB cable connections use [`TcpDirectTransport::physical`] (no extra
/// confirmation). Local Wi-Fi uses [`TcpDirectTransport::proximate`]
/// (requires mutual confirmation code).
pub struct TcpDirectTransport {
    stream: TcpStream,
    proximity: ProximityLevel,
}

impl TcpDirectTransport {
    /// Create a transport with `Physical` proximity (USB cable connection).
    pub fn physical(stream: TcpStream) -> Self {
        Self {
            stream,
            proximity: ProximityLevel::Physical,
        }
    }

    /// Create a transport with `Proximate` proximity (local Wi-Fi, no cable).
    pub fn proximate(stream: TcpStream) -> Self {
        Self {
            stream,
            proximity: ProximityLevel::Proximate,
        }
    }

    /// The proximity guarantee this transport provides.
    pub fn proximity_level(&self) -> ProximityLevel {
        self.proximity
    }

    /// Send a payload to the remote device using VXCH framing.
    pub fn send(&mut self, payload: &[u8]) -> Result<(), TcpTransportError> {
        send_payload(&mut self.stream, payload)
    }

    /// Receive a payload from the remote device using VXCH framing.
    pub fn recv(&mut self) -> Result<Vec<u8>, TcpTransportError> {
        recv_payload(&mut self.stream)
    }

    /// Exchange payloads bidirectionally.
    ///
    /// The `is_initiator` flag determines send/recv ordering to prevent
    /// deadlock (initiator sends first, responder receives first).
    pub fn exchange(
        &mut self,
        our_payload: &[u8],
        is_initiator: bool,
    ) -> Result<Vec<u8>, TcpTransportError> {
        exchange_payloads(&mut self.stream, our_payload, is_initiator)
    }
}

impl From<TcpTransportError> for ExchangeError {
    fn from(err: TcpTransportError) -> Self {
        match err {
            TcpTransportError::ConnectionClosed => ExchangeError::NetworkDisconnected,
            TcpTransportError::Io(_) => ExchangeError::NetworkDisconnected,
            TcpTransportError::InvalidMagic | TcpTransportError::UnsupportedVersion(_) => {
                ExchangeError::InvalidProtocolVersion
            }
            TcpTransportError::PayloadTooLarge(_) => {
                ExchangeError::InvalidState("direct transport payload too large".into())
            }
        }
    }
}
