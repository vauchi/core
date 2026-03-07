// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport negotiation: select the highest-priority transport
//! supported by both peers.

use super::caps::TransportCaps;
use super::channel::TransportType;

/// Ordered from highest to lowest priority for negotiation.
const TRANSPORT_MAP: &[(TransportCaps, TransportType)] = &[
    (TransportCaps::WIFI_AWARE, TransportType::WifiAware),
    (TransportCaps::TCP, TransportType::Tcp),
    (TransportCaps::BLE, TransportType::Ble),
    (TransportCaps::ANIMATED_QR, TransportType::AnimatedQr),
    (TransportCaps::STATIC_QR, TransportType::StaticQr),
];

/// Select highest-priority transport supported by both peers.
/// Falls back to `StaticQr` (always available).
pub fn negotiate_transport(ours: &TransportCaps, theirs: &TransportCaps) -> TransportType {
    let common = *ours & *theirs;
    for (cap, transport) in TRANSPORT_MAP {
        if common.contains(*cap) {
            return *transport;
        }
    }
    TransportType::StaticQr
}
