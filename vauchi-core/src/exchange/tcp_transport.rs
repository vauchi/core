// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! TCP-based exchange transport for USB cable and local network exchanges.
//!
//! Provides a simple length-prefixed protocol for exchanging payloads over
//! a TCP stream. Used by platform USB adapters (Android tethering, iOS
//! usbmuxd, desktop USB detection) and potentially local Wi-Fi exchange.
//!
//! Protocol frame: magic (4) + version (1) + length (4 BE) + payload.
//!
//! The protocol is symmetric — both sides run the same code. The initiator
//! connects, the responder listens. Initiator sends first; responder
//! receives first. This ordering avoids deadlock.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::time::Duration;

/// Maximum payload size (4 KB). Exchange payloads are ~300 bytes base64.
/// Tight limit provides defense-in-depth against malicious peers forcing
/// large allocations. Raise only if payloads genuinely grow.
const MAX_PAYLOAD_SIZE: u32 = 4_096;

/// Default I/O timeout for send/recv operations.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Vauchi exchange protocol magic bytes.
const PROTOCOL_MAGIC: &[u8; 4] = b"VXCH";

/// Protocol version.
const PROTOCOL_VERSION: u8 = 1;

/// Errors from TCP transport operations.
#[derive(Debug, thiserror::Error)]
pub enum TcpTransportError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Invalid protocol magic")]
    InvalidMagic,

    #[error("Unsupported protocol version: {0}")]
    UnsupportedVersion(u8),

    #[error("Payload too large: {0} bytes (max {MAX_PAYLOAD_SIZE})")]
    PayloadTooLarge(u32),

    #[error("Empty payload (length field is zero)")]
    EmptyPayload,
}

/// Send an exchange payload over a TCP stream.
///
/// Writes: magic (4) + version (1) + length (4 BE) + payload.
/// Sets a write timeout to avoid blocking indefinitely on unresponsive peers.
pub fn send_payload(stream: &mut TcpStream, payload: &[u8]) -> Result<(), TcpTransportError> {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    if len > MAX_PAYLOAD_SIZE {
        return Err(TcpTransportError::PayloadTooLarge(len));
    }
    if len == 0 {
        return Err(TcpTransportError::EmptyPayload);
    }

    stream.set_write_timeout(Some(DEFAULT_TIMEOUT))?;
    stream.write_all(PROTOCOL_MAGIC)?;
    stream.write_all(&[PROTOCOL_VERSION])?;
    stream.write_all(&len.to_be_bytes())?;
    stream.write_all(payload)?;
    stream.flush()?;
    Ok(())
}

/// Receive an exchange payload from a TCP stream.
///
/// Reads: magic (4) + version (1) + length (4 BE) + payload.
/// Sets a read timeout to avoid blocking indefinitely on unresponsive peers.
pub fn recv_payload(stream: &mut TcpStream) -> Result<Vec<u8>, TcpTransportError> {
    stream.set_read_timeout(Some(DEFAULT_TIMEOUT))?;

    // Read magic
    let mut magic = [0u8; 4];
    stream.read_exact(&mut magic)?;
    if &magic != PROTOCOL_MAGIC {
        return Err(TcpTransportError::InvalidMagic);
    }

    // Read version
    let mut version = [0u8; 1];
    stream.read_exact(&mut version)?;
    if version[0] != PROTOCOL_VERSION {
        return Err(TcpTransportError::UnsupportedVersion(version[0]));
    }

    // Read length
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_PAYLOAD_SIZE {
        return Err(TcpTransportError::PayloadTooLarge(len));
    }
    if len == 0 {
        return Err(TcpTransportError::EmptyPayload);
    }

    // Read payload
    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;
    Ok(payload)
}

/// Exchange payloads bidirectionally over a TCP stream.
///
/// Sends our payload and receives the peer's payload. The order is:
/// - Initiator (client): send first, then receive
/// - Responder (server): receive first, then send
///
/// This avoids deadlock since both sides agree on who sends first.
pub fn exchange_payloads(
    stream: &mut TcpStream,
    our_payload: &[u8],
    is_initiator: bool,
) -> Result<Vec<u8>, TcpTransportError> {
    if is_initiator {
        send_payload(stream, our_payload)?;
        recv_payload(stream)
    } else {
        let theirs = recv_payload(stream)?;
        send_payload(stream, our_payload)?;
        Ok(theirs)
    }
}
