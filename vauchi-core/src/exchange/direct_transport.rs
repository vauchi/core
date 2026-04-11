// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Direct transport trait for desktop-phone exchange.
//!
//! Provides a bidirectional byte channel abstraction for exchanges that
//! bypass the QR/NFC/BLE command/event flow and instead communicate directly
//! over a physical connection (USB cable, local network).
//!
//! This is NOT a replacement for the ADR-031 command/event protocol — it
//! complements it. QR/NFC/BLE continue using `ExchangeCommand`/`ExchangeHardwareEvent`.
//! Direct transports are for scenarios where core owns both ends of the
//! byte channel (e.g., TCP over USB tethering).

use super::error::ExchangeError;

/// Physical proximity guarantee provided by a transport.
///
/// Determines whether additional user confirmation is required during exchange.
/// `Physical` transports (USB cable, NFC tap) provide inherent proximity proof.
/// `Proximate` transports (BLE) require a mutual confirmation code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProximityLevel {
    /// Physical connection (USB cable, NFC tap) — no extra confirmation needed.
    Physical,
    /// Wireless proximity (BLE, local Wi-Fi) — requires mutual code confirmation.
    Proximate,
}

/// A bidirectional byte channel for direct desktop-phone exchange.
///
/// Implementors provide a reliable, ordered byte stream between two devices.
/// The exchange session uses this to send/receive exchange payloads without
/// involving the frontend's hardware abstraction layer.
///
/// # Contract
///
/// - `send` must deliver all bytes or return an error (no partial sends)
/// - `recv` must return a complete framed message or an error
/// - Implementations handle framing internally (e.g., length-prefixed)
/// - Both sides of the exchange run the same trait — the `is_initiator` flag
///   on `exchange` determines send/recv ordering to avoid deadlock
pub trait DirectTransport: Send + Sync {
    /// The proximity guarantee this transport provides.
    fn proximity_level(&self) -> ProximityLevel;

    /// Send a payload to the remote device.
    fn send(&mut self, payload: &[u8]) -> Result<(), ExchangeError>;

    /// Receive a payload from the remote device.
    fn recv(&mut self) -> Result<Vec<u8>, ExchangeError>;

    /// Exchange payloads bidirectionally.
    ///
    /// Sends our payload and receives theirs. The `is_initiator` flag
    /// determines ordering (initiator sends first) to prevent deadlock.
    ///
    /// Default implementation uses `send`/`recv` with role-based ordering.
    fn exchange(
        &mut self,
        our_payload: &[u8],
        is_initiator: bool,
    ) -> Result<Vec<u8>, ExchangeError> {
        if is_initiator {
            self.send(our_payload)?;
            self.recv()
        } else {
            let theirs = self.recv()?;
            self.send(our_payload)?;
            Ok(theirs)
        }
    }
}
