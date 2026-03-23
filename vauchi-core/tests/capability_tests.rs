// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Tests for Device Capability and Feature Gating
//!
//! Verifies that:
//! - DeviceCapabilities correctly represents hardware state
//! - FeatureGate gates features based on device hardware
//! - Runtime state (battery, network, storage) gates actions
//! - available_exchanges() returns only supported transports
//! - Serde roundtrip works for capability types

use vauchi_core::capability::*;
use vauchi_core::exchange::AudioCapability;

// ===== Mock RuntimeStateProvider for testing =====

struct MockRuntimeState {
    battery: u8,
    online: bool,
    storage_mb: u64,
}

impl MockRuntimeState {
    fn healthy() -> Self {
        MockRuntimeState {
            battery: 80,
            online: true,
            storage_mb: 1024,
        }
    }

    fn low_battery() -> Self {
        MockRuntimeState {
            battery: 3,
            online: true,
            storage_mb: 1024,
        }
    }

    fn warning_battery() -> Self {
        MockRuntimeState {
            battery: 12,
            online: true,
            storage_mb: 1024,
        }
    }

    fn offline() -> Self {
        MockRuntimeState {
            battery: 80,
            online: false,
            storage_mb: 1024,
        }
    }

    fn low_storage() -> Self {
        MockRuntimeState {
            battery: 80,
            online: true,
            storage_mb: 5,
        }
    }
}

impl RuntimeStateProvider for MockRuntimeState {
    fn is_online(&self) -> bool {
        self.online
    }

    fn connection_type(&self) -> ConnectionType {
        if self.online {
            ConnectionType::WiFi
        } else {
            ConnectionType::Offline
        }
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

fn make_caps(nfc: bool, ble: bool, camera: bool) -> DeviceCapabilities {
    DeviceCapabilities {
        has_nfc: nfc,
        has_ble: ble,
        has_camera: camera,
        audio: AudioCapability::None,
        has_biometrics: false,
        biometric_type: None,
        has_secure_enclave: false,
        platform: Platform::Unknown,
    }
}

fn make_gate(
    caps: DeviceCapabilities,
    runtime: impl RuntimeStateProvider + 'static,
) -> FeatureGate {
    FeatureGate::new(caps, Box::new(runtime))
}

// ===== Platform tests =====

// @scenario: device_capabilities :: Platform detection
#[test]
fn test_platform_default_is_unknown() {
    let platform = Platform::default();
    assert_eq!(platform, Platform::Unknown);
}

// @scenario: device_capabilities :: Platform detection
#[test]
fn test_platform_variants_distinct() {
    assert_ne!(Platform::Android, Platform::Ios);
    assert_ne!(Platform::Android, Platform::Desktop);
    assert_ne!(Platform::Ios, Platform::Web);
    assert_ne!(Platform::Desktop, Platform::Unknown);
}

// @scenario: device_capabilities :: Platform serialization
#[test]
fn test_platform_serde_roundtrip() {
    let platforms = vec![
        Platform::Android,
        Platform::Ios,
        Platform::Desktop,
        Platform::Web,
        Platform::Unknown,
    ];

    for platform in platforms {
        let json = serde_json::to_string(&platform).expect("serialize");
        let deserialized: Platform = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(platform, deserialized);
    }
}

// ===== DeviceCapabilities tests =====

// @scenario: device_capabilities :: Capabilities serialization
#[test]
fn test_device_capabilities_serde_roundtrip() {
    let caps = DeviceCapabilities {
        has_nfc: true,
        has_ble: true,
        has_camera: true,
        audio: AudioCapability::Full,
        has_biometrics: true,
        biometric_type: Some(BiometricType::FaceId),
        has_secure_enclave: true,
        platform: Platform::Ios,
    };

    let json = serde_json::to_string(&caps).expect("serialize");
    let deserialized: DeviceCapabilities = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(deserialized.platform, Platform::Ios);
    assert!(deserialized.has_nfc);
    assert!(deserialized.has_biometrics);
}

// ===== FeatureGate feature availability tests =====

// @scenario: device_capabilities :: QR display always available
#[test]
fn test_qr_display_always_available() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::QrDisplay),
        FeatureStatus::Available
    );
}

// @scenario: device_capabilities :: QR scan requires camera
#[test]
fn test_qr_scan_requires_camera() {
    let no_camera = make_caps(false, false, false);
    let gate = make_gate(no_camera, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::QrScan),
        FeatureStatus::Unavailable
    );

    let with_camera = make_caps(false, false, true);
    let gate = make_gate(with_camera, MockRuntimeState::healthy());
    assert_eq!(gate.is_available(Feature::QrScan), FeatureStatus::Available);
}

// @scenario: device_capabilities :: NFC exchange requires NFC hardware
#[test]
fn test_nfc_exchange_requires_nfc() {
    let no_nfc = make_caps(false, false, false);
    let gate = make_gate(no_nfc, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::NfcExchange),
        FeatureStatus::Unavailable
    );

    let with_nfc = make_caps(true, false, false);
    let gate = make_gate(with_nfc, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::NfcExchange),
        FeatureStatus::Available
    );
}

// @scenario: device_capabilities :: BLE exchange requires BLE hardware
#[test]
fn test_ble_exchange_requires_ble() {
    let no_ble = make_caps(false, false, false);
    let gate = make_gate(no_ble, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::BleExchange),
        FeatureStatus::Unavailable
    );

    let with_ble = make_caps(false, true, false);
    let gate = make_gate(with_ble, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::BleExchange),
        FeatureStatus::Available
    );
}

// @scenario: device_capabilities :: Mesh mode requires BLE
#[test]
fn test_mesh_mode_requires_ble() {
    let no_ble = make_caps(false, false, false);
    let gate = make_gate(no_ble, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::MeshMode),
        FeatureStatus::Unavailable
    );

    let with_ble = make_caps(false, true, false);
    let gate = make_gate(with_ble, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::MeshMode),
        FeatureStatus::Available
    );
}

// @scenario: device_capabilities :: Biometric unlock requires biometric hardware
#[test]
fn test_biometric_unlock_requires_biometrics() {
    let no_bio = make_caps(false, false, false);
    let gate = make_gate(no_bio, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::BiometricUnlock),
        FeatureStatus::Unavailable
    );

    let mut with_bio = make_caps(false, false, false);
    with_bio.has_biometrics = true;
    with_bio.biometric_type = Some(BiometricType::Fingerprint);
    let gate = make_gate(with_bio, MockRuntimeState::healthy());
    assert_eq!(
        gate.is_available(Feature::BiometricUnlock),
        FeatureStatus::Available
    );
}

// ===== FeatureGate action gating tests =====

// @scenario: device_capabilities :: Exchange blocked at critical battery
#[test]
fn test_exchange_blocked_at_critical_battery() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::low_battery());
    let status = gate.can_perform(Action::Exchange);
    assert!(matches!(status, ActionStatus::Blocked { .. }));
}

// @scenario: device_capabilities :: Exchange allowed at normal battery
#[test]
fn test_exchange_allowed_at_normal_battery() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::healthy());
    let status = gate.can_perform(Action::Exchange);
    assert_eq!(status, ActionStatus::Allowed);
}

// @scenario: device_capabilities :: Relay sync blocked when offline
#[test]
fn test_relay_sync_blocked_when_offline() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::offline());
    let status = gate.can_perform(Action::RelaySync);
    assert!(matches!(status, ActionStatus::Blocked { .. }));
}

// @scenario: device_capabilities :: Relay sync allowed when online
#[test]
fn test_relay_sync_allowed_when_online() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::healthy());
    let status = gate.can_perform(Action::RelaySync);
    assert_eq!(status, ActionStatus::Allowed);
}

// @scenario: device_capabilities :: Sync blocked at low storage
#[test]
fn test_sync_blocked_at_low_storage() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::low_storage());
    let status = gate.can_perform(Action::Sync);
    assert!(matches!(status, ActionStatus::Blocked { .. }));
}

// @scenario: device_capabilities :: Mesh relay blocked at critical battery
#[test]
fn test_mesh_relay_blocked_at_critical_battery() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::low_battery());
    let status = gate.can_perform(Action::MeshRelay);
    assert!(matches!(status, ActionStatus::Blocked { .. }));
}

// @scenario: device_capabilities :: Mesh relay warning at low battery
#[test]
fn test_mesh_relay_warning_at_low_battery() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::warning_battery());
    let status = gate.can_perform(Action::MeshRelay);
    assert!(matches!(status, ActionStatus::Warning { .. }));
}

// ===== available_exchanges tests =====

// @scenario: device_capabilities :: Available exchanges for minimal device
#[test]
fn test_available_exchanges_minimal_device() {
    let caps = make_caps(false, false, false);
    let gate = make_gate(caps, MockRuntimeState::healthy());
    let exchanges = gate.available_exchanges();
    // Should only have QrDisplay (no camera, no NFC, no BLE)
    assert_eq!(exchanges.len(), 1);
    assert!(exchanges.contains(&Feature::QrDisplay));
}

// @scenario: device_capabilities :: Available exchanges for full device
#[test]
fn test_available_exchanges_full_device() {
    let caps = make_caps(true, true, true);
    let gate = make_gate(caps, MockRuntimeState::healthy());
    let exchanges = gate.available_exchanges();
    assert!(exchanges.contains(&Feature::QrDisplay));
    assert!(exchanges.contains(&Feature::QrScan));
    assert!(exchanges.contains(&Feature::NfcExchange));
    assert!(exchanges.contains(&Feature::BleExchange));
    assert_eq!(exchanges.len(), 4);
}

// @scenario: device_capabilities :: Available exchanges for iOS without NFC
#[test]
fn test_available_exchanges_ios_no_nfc() {
    let mut caps = make_caps(false, true, true);
    caps.platform = Platform::Ios;
    let gate = make_gate(caps, MockRuntimeState::healthy());
    let exchanges = gate.available_exchanges();
    assert!(exchanges.contains(&Feature::QrDisplay));
    assert!(exchanges.contains(&Feature::QrScan));
    assert!(!exchanges.contains(&Feature::NfcExchange));
    assert!(exchanges.contains(&Feature::BleExchange));
    assert_eq!(exchanges.len(), 3);
}

// ===== AudioCapability serde tests =====

// @scenario: device_capabilities :: Audio capability serialization
#[test]
fn test_audio_capability_serde_roundtrip() {
    let variants = vec![
        AudioCapability::Full,
        AudioCapability::EmitOnly,
        AudioCapability::ReceiveOnly,
        AudioCapability::None,
    ];

    for original in variants {
        let json = serde_json::to_string(&original).expect("serialize");
        let deserialized: AudioCapability = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(original, deserialized);
    }
}

// ===== BiometricType tests =====

// @scenario: device_capabilities :: Biometric type serialization
#[test]
fn test_biometric_type_serde_roundtrip() {
    let types = vec![
        BiometricType::Fingerprint,
        BiometricType::FaceId,
        BiometricType::Iris,
    ];

    for bt in types {
        let json = serde_json::to_string(&bt).expect("serialize");
        let deserialized: BiometricType = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(bt, deserialized);
    }
}

// ===== ConnectionType tests =====

// @scenario: device_capabilities :: Connection type variants
#[test]
fn test_connection_type_variants() {
    let variants = [
        ConnectionType::WiFi,
        ConnectionType::Cellular,
        ConnectionType::Ethernet,
        ConnectionType::Offline,
    ];
    // All variants should be constructible and distinct
    for (i, a) in variants.iter().enumerate() {
        for (j, b) in variants.iter().enumerate() {
            if i != j {
                assert_ne!(a, b);
            }
        }
    }
}

// ===== Property-based tests =====

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
            let runtime = MockRuntimeState::healthy();
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
            let runtime = MockRuntimeState::healthy();
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

        /// NFC exchange availability should match has_nfc capability.
        #[test]
        fn prop_nfc_matches_capability(caps in arb_device_capabilities()) {
            let runtime = MockRuntimeState::healthy();
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

        /// BLE exchange availability should match has_ble capability.
        #[test]
        fn prop_ble_matches_capability(caps in arb_device_capabilities()) {
            let runtime = MockRuntimeState::healthy();
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
