// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::AudioCapability;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::{ExchangeId, ExchangeMode, check_mode_availability, recommend_mode};

fn arb_exchange_mode() -> impl Strategy<Value = ExchangeMode> {
    prop_oneof![
        Just(ExchangeMode::Glance),
        Just(ExchangeMode::Hover),
        Just(ExchangeMode::Bump),
        Just(ExchangeMode::Shake),
        Just(ExchangeMode::Magic),
        Just(ExchangeMode::TapTap),
        Just(ExchangeMode::TapHoverShake),
        Just(ExchangeMode::Link),
    ]
}

fn arb_audio() -> impl Strategy<Value = AudioCapability> {
    prop_oneof![
        Just(AudioCapability::Full),
        Just(AudioCapability::EmitOnly),
        Just(AudioCapability::ReceiveOnly),
        Just(AudioCapability::None),
    ]
}

fn arb_capabilities() -> impl Strategy<Value = DeviceCapabilities> {
    (
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        arb_audio(),
    )
        .prop_map(|(cam, ble, nfc, accel, inet, audio)| DeviceCapabilities {
            has_camera: cam,
            has_ble: ble,
            has_nfc: nfc,
            has_accelerometer: accel,
            has_internet: inet,
            audio,
            ..Default::default()
        })
}

proptest! {
// @internal
    #[test]
    fn recommend_always_returns_a_mode(caps in arb_capabilities()) {
        let mode = recommend_mode(&caps);
        // every recommended mode must have a positive timeout
        prop_assert!(mode.config().timeout.as_secs() > 0);
    }

// @internal
    #[test]
    fn exchange_id_roundtrip(bytes in any::<[u8; 32]>()) {
        let id = ExchangeId::from_bytes(bytes);
        let restored = ExchangeId::from_bytes(*id.as_bytes());
        prop_assert_eq!(id, restored);
    }

// @internal
    #[test]
    fn mode_config_timeout_positive(mode in arb_exchange_mode()) {
        let config = mode.config();
        prop_assert!(config.timeout.as_secs() > 0);
    }

// @internal
    #[test]
    fn check_availability_never_panics(
        mode in arb_exchange_mode(),
        caps in arb_capabilities(),
    ) {
        // Availability check must return a valid variant for any mode/caps combination
        let avail = check_mode_availability(mode, &caps);
        // Either Available or Unavailable — the enum has no invalid states
        let is_valid = matches!(
            avail,
            vauchi_core::exchange::ModeAvailability::Available
                | vauchi_core::exchange::ModeAvailability::Unavailable { .. }
                | vauchi_core::exchange::ModeAvailability::Degraded { .. }
        );
        prop_assert!(is_valid);
    }
}
