// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use proptest::prelude::*;
use vauchi_core::exchange::capability::types::DeviceCapabilities;
use vauchi_core::exchange::mode::{DataTransport, ExchangeContext, ProximityMethod};
use vauchi_core::exchange::{
    AudioCapability, ExchangeId, ExchangeMode, ExchangeRecord, ExchangeTrustLevel, ProximityResult,
    check_mode_availability, recommend_mode,
};

fn arb_exchange_mode() -> impl Strategy<Value = ExchangeMode> {
    prop_oneof![
        Just(ExchangeMode::Glance),
        Just(ExchangeMode::Hover),
        Just(ExchangeMode::Bump),
        Just(ExchangeMode::Shake),
        Just(ExchangeMode::Magic),
        Just(ExchangeMode::TapTap),
        Just(ExchangeMode::TapHoverShake),
        Just(ExchangeMode::Broadcast),
        Just(ExchangeMode::Web),
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
    #[test]
    fn trust_score_always_in_zero_one(
        confidence in 0.0f64..=1.0,
        relay_fb in any::<bool>(),
    ) {
        let record = ExchangeRecord {
            mode: ExchangeMode::Hover,
            context: ExchangeContext::InPerson,
            transport_used: DataTransport::QrMultiStage,
            relay_fallback: relay_fb,
            proximity_results: vec![ProximityResult {
                method: ProximityMethod::Audio,
                confidence,
                succeeded: true,
            }],
            timestamp: 0,
            reverifications: vec![],
        };
        let score = record.trust_score();
        prop_assert!(score >= 0.0);
        prop_assert!(score <= 1.0);
    }

    #[test]
    fn trust_level_covers_all_scores(score in 0.0f64..=1.0) {
        let level = ExchangeTrustLevel::from_score(score);
        // display_text must return a non-empty string for every level
        prop_assert!(!level.display_text().is_empty());
    }

    #[test]
    fn recommend_always_returns_a_mode(caps in arb_capabilities()) {
        let mode = recommend_mode(&caps);
        // every recommended mode must have a positive timeout
        prop_assert!(mode.config().timeout.as_secs() > 0);
    }

    #[test]
    fn exchange_id_roundtrip(bytes in any::<[u8; 32]>()) {
        let id = ExchangeId::from_bytes(bytes);
        let restored = ExchangeId::from_bytes(*id.as_bytes());
        prop_assert_eq!(id, restored);
    }

    #[test]
    fn mode_config_timeout_positive(mode in arb_exchange_mode()) {
        let config = mode.config();
        prop_assert!(config.timeout.as_secs() > 0);
    }

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
