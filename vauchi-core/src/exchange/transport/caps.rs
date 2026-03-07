// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport capability bitfield for negotiation.
//!
//! Encoded as 2 bytes in QR/NFC payloads. Backward compatible:
//! v2 peers send 0 caps, v3 peers append the bitfield after existing payload.

use serde::{Deserialize, Serialize};

bitflags::bitflags! {
    /// Advertised transport capabilities for peer negotiation.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub struct TransportCaps: u16 {
        /// Static QR code (always available)
        const STATIC_QR    = 0b0000_0001;
        /// Animated QR (frame-sequenced, higher throughput)
        const ANIMATED_QR  = 0b0000_0010;
        /// Bluetooth Low Energy (GATT)
        const BLE          = 0b0000_0100;
        /// WiFi Aware / NAN (peer-to-peer WiFi)
        const WIFI_AWARE   = 0b0000_1000;
        /// NFC tap trigger (initiates exchange on another transport)
        const NFC_TRIGGER  = 0b0001_0000;
        /// TCP (desktop, USB tethering)
        const TCP          = 0b0010_0000;
        // bits 6-15 reserved for future transports
    }
}

impl TransportCaps {
    /// Serialize to 2-byte big-endian for wire format.
    pub fn to_bytes(self) -> [u8; 2] {
        self.bits().to_be_bytes()
    }

    /// Deserialize from 2-byte big-endian wire format.
    /// Unknown bits are silently ignored (forward compatibility).
    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        Self::from_bits_truncate(u16::from_be_bytes(bytes))
    }
}
