// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A connected BLE state keeps its peer token through serde, and a token
//! of the wrong length or non-hex is refused (vauchi/private#522).

use vauchi_core::exchange::BLEExchangeState;

// @internal
#[test]
fn a_connected_state_keeps_its_peer_token() {
    let state = BLEExchangeState::Connected {
        peer_token: [7u8; 32],
        peer_device_id: "peer".into(),
    };

    let json = serde_json::to_string(&state).unwrap();
    let restored: BLEExchangeState = serde_json::from_str(&json).unwrap();

    assert!(json.contains(&"07".repeat(32)), "{json}");
    match restored {
        BLEExchangeState::Connected {
            peer_token,
            peer_device_id,
        } => {
            assert_eq!(peer_token, [7u8; 32]);
            assert_eq!(peer_device_id, "peer");
        }
        other => panic!("expected Connected, got {other:?}"),
    }
}

// @internal
#[test]
fn a_token_that_is_not_32_hex_bytes_is_refused() {
    for token in ["0707", "zz".repeat(32).as_str()] {
        let json =
            format!(r#"{{"state":"connected","peer_token":"{token}","peer_device_id":"peer"}}"#);
        assert!(
            serde_json::from_str::<BLEExchangeState>(&json).is_err(),
            "{token}"
        );
    }
}
