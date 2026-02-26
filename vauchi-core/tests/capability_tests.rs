// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for the device capability and feature gating module.
//!
//! These tests verify that hardware capabilities and runtime state
//! correctly gate feature availability per the lean installation design.

use vauchi_core::capability::{
    Action, ActionStatus, BiometricType, ConnectionType, DeviceCapabilities, Feature, FeatureGate,
    FeatureStatus, Platform, RuntimeStateProvider,
};
use vauchi_core::exchange::AudioCapability;

// === Test helper: Configurable RuntimeStateProvider ===

/// A test-only runtime state provider with configurable values.
struct TestRuntimeState {
    online: bool,
    connection: ConnectionType,
    battery: u8,
    storage_mb: u64,
}

impl Default for TestRuntimeState {
    fn default() -> Self {
        Self {
            online: true,
            connection: ConnectionType::WiFi,
            battery: 80,
            storage_mb: 500,
        }
    }
}

impl RuntimeStateProvider for TestRuntimeState {
    fn is_online(&self) -> bool {
        self.online
    }

    fn connection_type(&self) -> ConnectionType {
        self.connection.clone()
    }

    fn battery_level(&self) -> u8 {
        self.battery
    }

    fn is_battery_low(&self) -> bool {
        self.battery < 20
    }

    fn available_storage_mb(&self) -> u64 {
        self.storage_mb
    }
}

/// Helper: Create full-capability device with healthy runtime state.
fn full_capabilities() -> DeviceCapabilities {
    DeviceCapabilities {
        has_nfc: true,
        has_ble: true,
        has_camera: true,
        audio: AudioCapability::Full,
        has_biometrics: true,
        biometric_type: Some(BiometricType::Fingerprint),
        has_secure_enclave: true,
        platform: Platform::Android,
    }
}

fn healthy_runtime() -> TestRuntimeState {
    TestRuntimeState::default()
}

// === FeatureGate: Hardware capability gating ===

#[test]
fn test_feature_gate_no_nfc_disables_nfc_exchange() {
    let caps = DeviceCapabilities {
        has_nfc: false,
        ..full_capabilities()
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.is_available(Feature::NfcExchange);
    assert_eq!(status, FeatureStatus::Unavailable);
}

#[test]
fn test_feature_gate_no_ble_disables_ble_and_mesh() {
    let caps = DeviceCapabilities {
        has_ble: false,
        ..full_capabilities()
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    assert_eq!(
        gate.is_available(Feature::BleExchange),
        FeatureStatus::Unavailable
    );
    assert_eq!(
        gate.is_available(Feature::MeshMode),
        FeatureStatus::Unavailable
    );
}

#[test]
fn test_feature_gate_no_camera_disables_qr_scan() {
    let caps = DeviceCapabilities {
        has_camera: false,
        ..full_capabilities()
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    assert_eq!(
        gate.is_available(Feature::QrScan),
        FeatureStatus::Unavailable
    );
}

#[test]
fn test_feature_gate_qr_display_always_available() {
    // Even with zero capabilities, QR display must work.
    let caps = DeviceCapabilities {
        has_nfc: false,
        has_ble: false,
        has_camera: false,
        audio: AudioCapability::None,
        has_biometrics: false,
        biometric_type: None,
        has_secure_enclave: false,
        platform: Platform::Android,
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    assert_eq!(
        gate.is_available(Feature::QrDisplay),
        FeatureStatus::Available
    );
}

#[test]
fn test_feature_gate_biometric_unlock_requires_biometrics() {
    let caps = DeviceCapabilities {
        has_biometrics: false,
        biometric_type: None,
        ..full_capabilities()
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    assert_eq!(
        gate.is_available(Feature::BiometricUnlock),
        FeatureStatus::Unavailable
    );
}

#[test]
fn test_feature_gate_biometric_unlock_available_with_biometrics() {
    let caps = full_capabilities(); // has_biometrics: true
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    assert_eq!(
        gate.is_available(Feature::BiometricUnlock),
        FeatureStatus::Available
    );
}

// === FeatureGate: Runtime state gating ===

#[test]
fn test_feature_gate_low_battery_blocks_exchange() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        battery: 4, // < 5% threshold
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::Exchange);
    assert_eq!(
        status,
        ActionStatus::Blocked {
            reason: "Battery too low for exchange (< 5%)".to_string(),
        }
    );
}

#[test]
fn test_feature_gate_low_battery_warns_mesh() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        battery: 15, // < 20% but >= 5%
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::MeshRelay);
    assert_eq!(
        status,
        ActionStatus::Warning {
            reason: "Battery low \u{2014} mesh mode will drain battery faster".to_string(),
        }
    );
}

#[test]
fn test_feature_gate_offline_disables_relay() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        online: false,
        connection: ConnectionType::Offline,
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::RelaySync);
    assert_eq!(
        status,
        ActionStatus::Blocked {
            reason: "No network connection available".to_string(),
        }
    );
}

#[test]
fn test_feature_gate_low_storage_blocks_sync() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        storage_mb: 5, // < 10MB threshold
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::Sync);
    assert_eq!(
        status,
        ActionStatus::Blocked {
            reason: "Insufficient storage for sync (< 10 MB)".to_string(),
        }
    );
}

// === FeatureGate: Available exchanges ===

#[test]
fn test_feature_gate_minimum_one_exchange_always() {
    // Even with everything disabled, QR display must be in available exchanges.
    let caps = DeviceCapabilities {
        has_nfc: false,
        has_ble: false,
        has_camera: false,
        audio: AudioCapability::None,
        has_biometrics: false,
        biometric_type: None,
        has_secure_enclave: false,
        platform: Platform::Android,
    };
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let exchanges = gate.available_exchanges();
    assert!(
        !exchanges.is_empty(),
        "At least QR display must be available"
    );
    assert!(
        exchanges.contains(&Feature::QrDisplay),
        "QR display must always be in available exchanges"
    );
}

#[test]
fn test_feature_gate_all_capabilities_all_available() {
    let caps = full_capabilities();
    let runtime = healthy_runtime();
    let gate = FeatureGate::new(caps, Box::new(runtime));

    // All exchange features should be available
    assert_eq!(
        gate.is_available(Feature::QrDisplay),
        FeatureStatus::Available
    );
    assert_eq!(gate.is_available(Feature::QrScan), FeatureStatus::Available);
    assert_eq!(
        gate.is_available(Feature::NfcExchange),
        FeatureStatus::Available
    );
    assert_eq!(
        gate.is_available(Feature::BleExchange),
        FeatureStatus::Available
    );
    assert_eq!(
        gate.is_available(Feature::MeshMode),
        FeatureStatus::Available
    );
    assert_eq!(
        gate.is_available(Feature::BiometricUnlock),
        FeatureStatus::Available
    );

    // All exchange features should be in the list
    let exchanges = gate.available_exchanges();
    assert!(exchanges.contains(&Feature::QrDisplay));
    assert!(exchanges.contains(&Feature::QrScan));
    assert!(exchanges.contains(&Feature::NfcExchange));
    assert!(exchanges.contains(&Feature::BleExchange));
}

// === Boundary tests ===

#[test]
fn test_feature_gate_battery_exactly_five_allows_exchange() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        battery: 5, // exactly at threshold -- should be allowed
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::Exchange);
    assert_eq!(status, ActionStatus::Allowed);
}

#[test]
fn test_feature_gate_battery_exactly_twenty_no_mesh_warning() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        battery: 20, // exactly at threshold -- should not warn
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::MeshRelay);
    assert_eq!(status, ActionStatus::Allowed);
}

#[test]
fn test_feature_gate_storage_exactly_ten_allows_sync() {
    let caps = full_capabilities();
    let runtime = TestRuntimeState {
        storage_mb: 10, // exactly at threshold -- should be allowed
        ..Default::default()
    };
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::Sync);
    assert_eq!(status, ActionStatus::Allowed);
}

#[test]
fn test_feature_gate_online_allows_relay() {
    let caps = full_capabilities();
    let runtime = healthy_runtime(); // online: true
    let gate = FeatureGate::new(caps, Box::new(runtime));

    let status = gate.can_perform(Action::RelaySync);
    assert_eq!(status, ActionStatus::Allowed);
}

// === DeviceCapabilities serialization ===

#[test]
fn test_device_capabilities_serialization_roundtrip() {
    let caps = full_capabilities();
    let json = serde_json::to_string(&caps).expect("serialize");
    let deserialized: DeviceCapabilities = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(caps.has_nfc, deserialized.has_nfc);
    assert_eq!(caps.has_ble, deserialized.has_ble);
    assert_eq!(caps.has_camera, deserialized.has_camera);
    assert_eq!(caps.audio, deserialized.audio);
    assert_eq!(caps.has_biometrics, deserialized.has_biometrics);
    assert_eq!(caps.biometric_type, deserialized.biometric_type);
    assert_eq!(caps.has_secure_enclave, deserialized.has_secure_enclave);
    assert_eq!(caps.platform, deserialized.platform);
}

#[test]
fn test_device_capabilities_default_values() {
    // Deserialize with missing fields should use defaults (serde(default))
    let json = "{}";
    let caps: DeviceCapabilities = serde_json::from_str(json).expect("deserialize with defaults");

    assert!(!caps.has_nfc);
    assert!(!caps.has_ble);
    assert!(!caps.has_camera);
    assert_eq!(caps.audio, AudioCapability::None);
    assert!(!caps.has_biometrics);
    assert_eq!(caps.biometric_type, None);
    assert!(!caps.has_secure_enclave);
    assert_eq!(caps.platform, Platform::Unknown);
}

// === Property-based tests ===

#[cfg(test)]
mod proptest_capability {
    use super::*;
    use proptest::prelude::*;

    fn arb_audio_capability() -> impl Strategy<Value = AudioCapability> {
        prop_oneof![
            Just(AudioCapability::Full),
            Just(AudioCapability::EmitOnly),
            Just(AudioCapability::ReceiveOnly),
            Just(AudioCapability::None),
        ]
    }

    fn arb_biometric_type() -> impl Strategy<Value = Option<BiometricType>> {
        prop_oneof![
            Just(None),
            Just(Some(BiometricType::Fingerprint)),
            Just(Some(BiometricType::FaceId)),
            Just(Some(BiometricType::Iris)),
        ]
    }

    fn arb_platform() -> impl Strategy<Value = Platform> {
        prop_oneof![
            Just(Platform::Android),
            Just(Platform::Ios),
            Just(Platform::Desktop),
            Just(Platform::Web),
            Just(Platform::Unknown),
        ]
    }

    fn arb_device_capabilities() -> impl Strategy<Value = DeviceCapabilities> {
        (
            any::<bool>(), // has_nfc
            any::<bool>(), // has_ble
            any::<bool>(), // has_camera
            arb_audio_capability(),
            any::<bool>(), // has_biometrics
            arb_biometric_type(),
            any::<bool>(), // has_secure_enclave
            arb_platform(),
        )
            .prop_map(
                |(
                    has_nfc,
                    has_ble,
                    has_camera,
                    audio,
                    has_biometrics,
                    biometric_type,
                    has_secure_enclave,
                    platform,
                )| {
                    DeviceCapabilities {
                        has_nfc,
                        has_ble,
                        has_camera,
                        audio,
                        has_biometrics,
                        biometric_type,
                        has_secure_enclave,
                        platform,
                    }
                },
            )
    }

    proptest! {
        /// QR display must always be available regardless of device capabilities.
        #[test]
        fn prop_qr_display_always_available(caps in arb_device_capabilities()) {
            let runtime = TestRuntimeState::default();
            let gate = FeatureGate::new(caps, Box::new(runtime));

            prop_assert_eq!(
                gate.is_available(Feature::QrDisplay),
                FeatureStatus::Available,
                "QR display must be available for any capability combination"
            );
        }

        /// available_exchanges() must always return at least one feature (QR display).
        #[test]
        fn prop_at_least_one_exchange_method(caps in arb_device_capabilities()) {
            let runtime = TestRuntimeState::default();
            let gate = FeatureGate::new(caps, Box::new(runtime));

            let exchanges = gate.available_exchanges();
            prop_assert!(
                !exchanges.is_empty(),
                "There must always be at least one available exchange method"
            );
            prop_assert!(
                exchanges.contains(&Feature::QrDisplay),
                "QR display must always be in available exchanges"
            );
        }

        /// NFC exchange should match has_nfc capability.
        #[test]
        fn prop_nfc_matches_capability(caps in arb_device_capabilities()) {
            let runtime = TestRuntimeState::default();
            let gate = FeatureGate::new(caps.clone(), Box::new(runtime));

            if caps.has_nfc {
                prop_assert_eq!(
                    gate.is_available(Feature::NfcExchange),
                    FeatureStatus::Available
                );
            } else {
                prop_assert_eq!(
                    gate.is_available(Feature::NfcExchange),
                    FeatureStatus::Unavailable
                );
            }
        }

        /// BLE exchange should match has_ble capability.
        #[test]
        fn prop_ble_matches_capability(caps in arb_device_capabilities()) {
            let runtime = TestRuntimeState::default();
            let gate = FeatureGate::new(caps.clone(), Box::new(runtime));

            if caps.has_ble {
                prop_assert_eq!(
                    gate.is_available(Feature::BleExchange),
                    FeatureStatus::Available
                );
            } else {
                prop_assert_eq!(
                    gate.is_available(Feature::BleExchange),
                    FeatureStatus::Unavailable
                );
            }
        }
    }
}
