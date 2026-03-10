// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Feature gate logic.
//!
//! Combines static device capabilities with dynamic runtime state
//! to determine which features and actions are available.

use super::runtime::RuntimeStateProvider;
use super::types::DeviceCapabilities;

/// Minimum battery level (%) required for exchange operations.
const BATTERY_CRITICAL_THRESHOLD: u8 = 5;

/// Battery level (%) below which mesh mode triggers a warning.
const BATTERY_LOW_THRESHOLD: u8 = 20;

/// Minimum available storage (MB) required for sync operations.
const STORAGE_MIN_MB: u64 = 10;

/// A feature that can be checked for availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Feature {
    /// Display a QR code for exchange (always available).
    QrDisplay,
    /// Scan a QR code for exchange (requires camera).
    QrScan,
    /// NFC-based exchange (requires NFC hardware).
    NfcExchange,
    /// BLE-based exchange (requires BLE hardware).
    BleExchange,
    /// Mesh relay mode (requires BLE hardware).
    MeshMode,
    /// Biometric unlock (requires biometric hardware).
    BiometricUnlock,
}

/// Result of checking feature availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeatureStatus {
    /// Feature is available and can be used.
    Available,
    /// Feature is not available (hardware not present).
    Unavailable,
}

/// An action that can be checked against runtime state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Perform a contact exchange (blocked when battery < 5%).
    Exchange,
    /// Use mesh relay mode (warned when battery < 20%).
    MeshRelay,
    /// Sync via relay (blocked when offline).
    RelaySync,
    /// Sync contact data (blocked when storage < 10 MB).
    Sync,
}

/// Result of checking whether an action can be performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionStatus {
    /// Action is allowed.
    Allowed,
    /// Action is allowed but with a warning.
    Warning { reason: String },
    /// Action is blocked.
    Blocked { reason: String },
}

/// Evaluates device capabilities and runtime state to gate features.
///
/// Created once with static capabilities, queries runtime state dynamically.
///
/// ## Invariant
///
/// `is_available(Feature::QrDisplay)` always returns `FeatureStatus::Available`.
/// `available_exchanges()` always contains at least `Feature::QrDisplay`.
pub struct FeatureGate {
    capabilities: DeviceCapabilities,
    runtime: Box<dyn RuntimeStateProvider>,
}

impl FeatureGate {
    /// Creates a new feature gate with the given capabilities and runtime provider.
    pub fn new(capabilities: DeviceCapabilities, runtime: Box<dyn RuntimeStateProvider>) -> Self {
        Self {
            capabilities,
            runtime,
        }
    }

    /// Checks whether a feature is available based on static device capabilities.
    pub fn is_available(&self, feature: Feature) -> FeatureStatus {
        match feature {
            // QR display is always available -- it only requires a screen.
            Feature::QrDisplay => FeatureStatus::Available,

            Feature::QrScan => {
                if self.capabilities.has_camera {
                    FeatureStatus::Available
                } else {
                    FeatureStatus::Unavailable
                }
            }

            Feature::NfcExchange => {
                if self.capabilities.has_nfc {
                    FeatureStatus::Available
                } else {
                    FeatureStatus::Unavailable
                }
            }

            Feature::BleExchange => {
                if self.capabilities.has_ble {
                    FeatureStatus::Available
                } else {
                    FeatureStatus::Unavailable
                }
            }

            Feature::MeshMode => {
                if self.capabilities.has_ble {
                    FeatureStatus::Available
                } else {
                    FeatureStatus::Unavailable
                }
            }

            Feature::BiometricUnlock => {
                if self.capabilities.has_biometrics {
                    FeatureStatus::Available
                } else {
                    FeatureStatus::Unavailable
                }
            }
        }
    }

    /// Checks whether a runtime-dependent action can be performed.
    ///
    /// Evaluates dynamic state (battery, network, storage) to determine
    /// if the action should be allowed, warned, or blocked.
    pub fn can_perform(&self, action: Action) -> ActionStatus {
        match action {
            Action::Exchange => {
                let battery = self.runtime.battery_level();
                if battery < BATTERY_CRITICAL_THRESHOLD {
                    ActionStatus::Blocked {
                        reason: "Battery too low for exchange (< 5%)".to_string(),
                    }
                } else {
                    ActionStatus::Allowed
                }
            }

            Action::MeshRelay => {
                let battery = self.runtime.battery_level();
                if battery < BATTERY_CRITICAL_THRESHOLD {
                    ActionStatus::Blocked {
                        reason: "Battery too low for exchange (< 5%)".to_string(),
                    }
                } else if battery < BATTERY_LOW_THRESHOLD {
                    ActionStatus::Warning {
                        reason: "Battery low \u{2014} mesh mode will drain battery faster"
                            .to_string(),
                    }
                } else {
                    ActionStatus::Allowed
                }
            }

            Action::RelaySync => {
                if !self.runtime.is_online() {
                    ActionStatus::Blocked {
                        reason: "No network connection available".to_string(),
                    }
                } else {
                    ActionStatus::Allowed
                }
            }

            Action::Sync => {
                let storage = self.runtime.available_storage_mb();
                if storage < STORAGE_MIN_MB {
                    ActionStatus::Blocked {
                        reason: "Insufficient storage for sync (< 10 MB)".to_string(),
                    }
                } else {
                    ActionStatus::Allowed
                }
            }
        }
    }

    /// Returns all exchange-related features that are currently available.
    ///
    /// Always includes `Feature::QrDisplay` (guaranteed invariant).
    pub fn available_exchanges(&self) -> Vec<Feature> {
        let exchange_features = [
            Feature::QrDisplay,
            Feature::QrScan,
            Feature::NfcExchange,
            Feature::BleExchange,
        ];

        exchange_features
            .into_iter()
            .filter(|f| self.is_available(f.clone()) == FeatureStatus::Available)
            .collect()
    }
}
