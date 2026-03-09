// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for WiFi Aware transport.
//!
//! Defines callback traits that native iOS/Android implement.

/// Callback trait that mobile platforms implement for WiFi Aware.
#[uniffi::export(callback_interface)]
pub trait MobileWifiAwareHandler: Send + Sync {
    /// Called when a peer is discovered nearby.
    fn on_peer_discovered(&self, peer_id: String, rssi: Option<i8>);
    /// Called when connected to a peer.
    fn on_connected(&self, peer_id: String);
    /// Called when data is received from a peer.
    fn on_data_received(&self, data: Vec<u8>);
    /// Called on error.
    fn on_error(&self, message: String);
}

/// WiFi Aware availability check result.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileWifiAwareStatus {
    pub is_available: bool,
    pub reason: Option<String>,
}

/// Check WiFi Aware availability (platform-dependent).
/// Returns status with reason if unavailable.
#[uniffi::export]
pub fn wifi_aware_check_availability() -> MobileWifiAwareStatus {
    // WiFi Aware availability is determined at the platform layer.
    // This returns a conservative default — the actual check happens
    // in the native iOS/Android code.
    MobileWifiAwareStatus {
        is_available: false,
        reason: Some("WiFi Aware availability must be checked on-device".into()),
    }
}
