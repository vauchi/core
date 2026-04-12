// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Types for direct transport exchange (USB cable / local network).
//!
//! Direct transports use the same ADR-031 command/event protocol as
//! QR/NFC/BLE. Core emits [`ExchangeCommand::DirectSend`] and receives
//! [`ExchangeHardwareEvent::DirectPayloadReceived`]. The actual TCP I/O
//! is performed by frontends using [`TcpDirectTransport`] or raw
//! [`tcp_transport`] functions.

/// Physical proximity guarantee provided by a transport.
///
/// Determines whether additional user confirmation is required during exchange.
/// `Physical` transports (USB cable, NFC tap) provide inherent proximity proof.
/// `Proximate` transports (BLE, local Wi-Fi) require a mutual confirmation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityLevel {
    /// Physical connection (USB cable, NFC tap) — no extra confirmation needed.
    Physical,
    /// Wireless proximity (BLE, local Wi-Fi) — requires mutual code confirmation.
    Proximate,
}
