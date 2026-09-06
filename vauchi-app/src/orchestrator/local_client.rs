// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Joiner-side broker for local device linking (ADR-070 Phase 1).
//!
//! The scanning device talks to the host's rendezvous over a socket instead
//! of the relay. Deliberately **not** `HttpTransport`: its `post_action` has
//! two live paths, and neither fits. OHTTP is meaningless peer-to-peer —
//! ADR-037 requires the gateway and relay to be distinct entities, which one
//! LAN device cannot be — and the direct path needs `allow_direct`, which
//! `check-no-allow-direct.sh` forbids in production because it leaks the
//! caller's source IP to the relay. That reasoning does not transfer to a
//! peer on your own segment, but the lint is blanket and weakening a
//! security lint to fit a convenience is the move CC-21 exists to stop.
//!
//! The address comes from a scanned QR, so it can point anywhere. Every
//! answer is therefore bounded the same way the host bounds requests
//! (DC-01): a hostile or confused peer must not be able to make the joiner
//! read without limit, nor hang it by accepting and never replying.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use vauchi_core::network::NetworkError;

use super::device_link_relay::DeviceLinkBroker;
use super::local_wire::{
    LocalRequest, LocalResponse, MAX_FRAME_BYTES, decode_response, encode_request,
};

/// A [`DeviceLinkBroker`] backed by a host rendezvous reached over TCP.
pub struct RemoteRendezvousBroker {
    addr: SocketAddr,
    connect_timeout: Duration,
    read_timeout: Duration,
}

impl RemoteRendezvousBroker {
    /// Talk to the rendezvous at `addr`.
    ///
    /// Both timeouts are required rather than defaulted: this runs on the
    /// machine's poll thread, and an address that accepts but never answers
    /// would otherwise stall the ceremony indefinitely.
    pub fn new(addr: SocketAddr, connect_timeout: Duration, read_timeout: Duration) -> Self {
        Self {
            addr,
            connect_timeout,
            read_timeout,
        }
    }

    /// One request, one connection: connect, write, half-close, read.
    fn round_trip(&self, request: &LocalRequest) -> Result<LocalResponse, NetworkError> {
        let mut stream = TcpStream::connect_timeout(&self.addr, self.connect_timeout)
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        stream
            .set_read_timeout(Some(self.read_timeout))
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;

        write_request(&mut stream, request)?;
        let reply = read_bounded(&mut stream)?;
        decode_response(&reply).map_err(|e| NetworkError::InvalidMessage(e.to_string()))
    }
}

/// A host that answers the wrong shape is a protocol violation, not a
/// refusal — surfaced distinctly so it cannot read as "the peer declined".
fn unexpected(response: &LocalResponse) -> NetworkError {
    NetworkError::InvalidMessage(match response {
        LocalResponse::Error { .. } => "rendezvous refused the request".into(),
        _ => "rendezvous answered the wrong shape".into(),
    })
}

impl DeviceLinkBroker for RemoteRendezvousBroker {
    /// `expires_secs` is ignored: the ceremony deadline belongs to the
    /// machine (ADR-035's QR window) and the host bounds its own listener,
    /// so there is nothing for the joiner to expire independently.
    fn exchange_offer(
        &self,
        payload_b64: &str,
        _expires_secs: Option<u64>,
    ) -> Result<String, NetworkError> {
        match self.round_trip(&LocalRequest::ExchangeOffer {
            payload: payload_b64.to_string(),
        })? {
            LocalResponse::Offered { code } => Ok(code),
            other => Err(unexpected(&other)),
        }
    }

    fn exchange_claim(&self, code: &str, response_b64: &str) -> Result<String, NetworkError> {
        match self.round_trip(&LocalRequest::ExchangeClaim {
            code: code.to_string(),
            response: response_b64.to_string(),
        })? {
            LocalResponse::Claimed { payload } => Ok(payload),
            other => Err(unexpected(&other)),
        }
    }

    fn exchange_complete(&self, code: &str) -> Result<Option<String>, NetworkError> {
        match self.round_trip(&LocalRequest::ExchangeComplete {
            code: code.to_string(),
        })? {
            LocalResponse::Polled { response } => Ok(response),
            other => Err(unexpected(&other)),
        }
    }
}

/// Read a bounded reply from `stream`.
fn read_bounded(stream: &mut TcpStream) -> Result<Vec<u8>, NetworkError> {
    let mut reply = Vec::new();
    Read::by_ref(stream)
        .take((MAX_FRAME_BYTES + 1) as u64)
        .read_to_end(&mut reply)
        .map_err(|e| NetworkError::ReceiveFailed(e.to_string()))?;
    if reply.len() > MAX_FRAME_BYTES {
        return Err(NetworkError::InvalidMessage("reply too large".into()));
    }
    Ok(reply)
}

/// Kept separate so the write path reads as one step in `round_trip`.
fn write_request(stream: &mut TcpStream, request: &LocalRequest) -> Result<(), NetworkError> {
    stream
        .write_all(&encode_request(request))
        .and_then(|()| stream.flush())
        .and_then(|()| stream.shutdown(std::net::Shutdown::Write))
        .map_err(|e| NetworkError::SendFailed(e.to_string()))
}
