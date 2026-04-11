// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Device linking over direct transports (USB cable / local network).
//!
//! Enables device linking without a camera to scan QR codes. The initiator
//! (existing device) sends the `DeviceLinkQR` data over a `DirectTransport`,
//! and the responder (new device) receives it and proceeds with the standard
//! request/response protocol — all over the same transport.
//!
//! Protocol (3 rounds over DirectTransport):
//! 1. Initiator → Responder: DeviceLinkQR data string
//! 2. Responder → Initiator: encrypted DeviceLinkRequest
//! 3. Initiator → Responder: encrypted DeviceLinkResponse

use super::super::direct_transport::DirectTransport;
use super::super::error::ExchangeError;
use super::qr::DeviceLinkQR;

/// Sends a `DeviceLinkQR` over a direct transport (initiator side, round 1).
///
/// The initiator calls this instead of displaying a QR code on screen.
/// The responder receives it via [`recv_device_link_qr`].
pub fn send_device_link_qr(
    transport: &mut dyn DirectTransport,
    qr: &DeviceLinkQR,
) -> Result<(), ExchangeError> {
    let data = qr.to_data_string();
    transport.send(data.as_bytes())
}

/// Receives a `DeviceLinkQR` from a direct transport (responder side, round 1).
///
/// The responder calls this instead of scanning a QR code with a camera.
/// Returns the parsed and validated QR (signature verified, not expired).
pub fn recv_device_link_qr(
    transport: &mut dyn DirectTransport,
) -> Result<DeviceLinkQR, ExchangeError> {
    let data = transport.recv()?;
    let data_str = String::from_utf8(data).map_err(|_| ExchangeError::InvalidQRFormat)?;
    let qr = DeviceLinkQR::from_data_string(&data_str)?;

    if qr.is_expired() {
        return Err(ExchangeError::DeviceLinkQRExpired);
    }
    if !qr.verify_signature() {
        return Err(ExchangeError::InvalidSignature);
    }

    Ok(qr)
}

/// Sends an encrypted request/response blob over a direct transport.
///
/// Used for rounds 2 (responder sends request) and 3 (initiator sends response).
pub fn send_encrypted_blob(
    transport: &mut dyn DirectTransport,
    blob: &[u8],
) -> Result<(), ExchangeError> {
    transport.send(blob)
}

/// Receives an encrypted request/response blob from a direct transport.
///
/// Used for rounds 2 (initiator receives request) and 3 (responder receives response).
pub fn recv_encrypted_blob(transport: &mut dyn DirectTransport) -> Result<Vec<u8>, ExchangeError> {
    transport.recv()
}
