// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

#![cfg(feature = "testing")]

use proptest::prelude::*;
use vauchi_core::exchange::transport::TransportType;
use vauchi_core::exchange::transport::caps::TransportCaps;
use vauchi_core::exchange::transport::negotiation::negotiate_transport;
use vauchi_core::exchange::transport::protocol::ExchangeProtocol;

proptest! {
// @internal
    #[test]
    fn caps_roundtrip(bits in 0u16..=0b1111_1111u16) {
        let caps = TransportCaps::from_bits_truncate(bits);
        let bytes = caps.to_bytes();
        let restored = TransportCaps::from_bytes(bytes);
        prop_assert_eq!(caps, restored);
    }

// @internal
    #[test]
    fn negotiation_always_returns_valid_type(
        ours_bits in 0u16..=0b1111_1111u16,
        theirs_bits in 0u16..=0b1111_1111u16,
    ) {
        let ours = TransportCaps::from_bits_truncate(ours_bits);
        let theirs = TransportCaps::from_bits_truncate(theirs_bits);
        let result = negotiate_transport(&ours, &theirs);
        prop_assert!(matches!(result,
            TransportType::WifiAware | TransportType::Ble |
            TransportType::AnimatedQr | TransportType::StaticQr |
            TransportType::Nfc | TransportType::Tcp
        ));
    }

// @internal
    #[test]
    fn negotiation_symmetric(
        ours_bits in 0u16..=0b1111_1111u16,
        theirs_bits in 0u16..=0b1111_1111u16,
    ) {
        let ours = TransportCaps::from_bits_truncate(ours_bits);
        let theirs = TransportCaps::from_bits_truncate(theirs_bits);
        let result_ab = negotiate_transport(&ours, &theirs);
        let result_ba = negotiate_transport(&theirs, &ours);
        prop_assert_eq!(result_ab, result_ba, "negotiation must be symmetric");
    }

// @internal
    #[test]
    fn encrypt_decrypt_roundtrip(data in proptest::collection::vec(any::<u8>(), 1..4096)) {
        let alice = ExchangeProtocol::new_random();
        let bob = ExchangeProtocol::new_random();
        let offer_a = alice.create_offer().unwrap();
        let offer_b = bob.create_offer().unwrap();
        let shared_a = alice.process_offer(&offer_b).unwrap();
        let shared_b = bob.process_offer(&offer_a).unwrap();
        // Both sides derive same key
        prop_assert_eq!(shared_a.as_bytes(), shared_b.as_bytes());
        // Encrypt with Alice's key, decrypt with Bob's key
        let encrypted = ExchangeProtocol::encrypt_card(&data, &shared_a).unwrap();
        let decrypted = ExchangeProtocol::decrypt_card(&encrypted, &shared_b).unwrap();
        prop_assert_eq!(data, decrypted);
    }
}
