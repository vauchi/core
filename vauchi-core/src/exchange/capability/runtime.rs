// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Runtime state provider trait.
//!
//! Defines the callback interface for querying dynamic device state
//! (battery, network, storage) at feature-gate evaluation time.

use serde::{Deserialize, Serialize};

/// Callback interface for querying dynamic device runtime state.
///
/// Implemented by the platform layer (iOS/Android/Desktop) to provide
/// real-time information about battery, network, and storage.
///
/// The `FeatureGate` calls these methods when evaluating whether
/// runtime-dependent actions (exchange, sync, relay) are allowed.
pub trait RuntimeStateProvider: Send + Sync {
    /// Returns true if the device has any network connectivity.
    fn is_online(&self) -> bool;

    /// Returns the current network connection type.
    fn connection_type(&self) -> ConnectionType;

    /// Returns the current battery level as a percentage (0-100).
    fn battery_level(&self) -> u8;

    /// Returns true if the battery is considered low (< 20%).
    fn is_battery_low(&self) -> bool;

    /// Returns available storage in megabytes.
    fn available_storage_mb(&self) -> u64;
}

/// Type of network connection available.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionType {
    /// Connected via WiFi.
    WiFi,
    /// Connected via cellular data.
    Cellular,
    /// Connected via Ethernet (desktop).
    Ethernet,
    /// No network connection.
    Offline,
}
