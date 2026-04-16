// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::BleCardPayload;

// @internal
#[test]
fn test_ble_card_payload_roundtrip() {
    let fields = vec![
        ("email".to_string(), "alice@example.com".to_string()),
        ("phone".to_string(), "+41791234567".to_string()),
    ];
    let avatar = Some(vec![0xDE, 0xAD, 0xBE, 0xEF]);

    let original = BleCardPayload::new(
        [0xAA; 32],
        "Alice".to_string(),
        [0xBB; 32],
        fields.clone(),
        avatar.clone(),
    );

    let bytes = original.to_bytes().expect("serialization should succeed");
    let restored = BleCardPayload::from_bytes(&bytes).expect("deserialization should succeed");

    assert_eq!(original.identity_key, restored.identity_key);
    assert_eq!(original.display_name, restored.display_name);
    assert_eq!(original.exchange_key, restored.exchange_key);
    assert_eq!(original.fields, restored.fields);
    assert_eq!(original.avatar, restored.avatar);
    assert_eq!(original.crc16, restored.crc16);
    assert!(restored.verify_crc16(), "CRC16 must verify after roundtrip");
}

// @internal
#[test]
fn test_ble_card_payload_crc16_detects_corruption() {
    let fields = vec![("email".to_string(), "bob@example.com".to_string())];
    let avatar = Some(vec![0x01, 0x02, 0x03]);

    let original = BleCardPayload::new([0x11; 32], "Bob".to_string(), [0x22; 32], fields, avatar);
    assert!(
        original.verify_crc16(),
        "CRC16 must verify on fresh payload"
    );

    // Serialize, corrupt one byte, deserialize, verify CRC fails
    let mut bytes = original.to_bytes().expect("serialization should succeed");
    bytes[0] ^= 0xFF; // flip a byte in the identity_key
    let corrupted = BleCardPayload::from_bytes(&bytes).expect("deserialization should still work");
    assert!(
        !corrupted.verify_crc16(),
        "CRC16 must fail after byte corruption"
    );
}

// @internal
#[test]
fn test_ble_card_payload_empty_fields_and_no_avatar() {
    let original = BleCardPayload::new([0x00; 32], "Empty".to_string(), [0x00; 32], vec![], None);

    let bytes = original
        .to_bytes()
        .expect("empty fields + no avatar should serialize");
    let restored = BleCardPayload::from_bytes(&bytes).expect("should deserialize");

    assert_eq!(restored.fields, Vec::<(String, String)>::new());
    assert_eq!(restored.avatar, None);
    assert!(
        restored.verify_crc16(),
        "CRC16 must verify for empty optional data"
    );
}

// @internal
#[test]
fn test_ble_card_payload_large_avatar() {
    let large_avatar = vec![0x42; 10 * 1024]; // 10KB

    let original = BleCardPayload::new(
        [0xFF; 32],
        "LargeAvatar".to_string(),
        [0xEE; 32],
        vec![("key".to_string(), "value".to_string())],
        Some(large_avatar.clone()),
    );

    let bytes = original.to_bytes().expect("large avatar should serialize");
    let restored = BleCardPayload::from_bytes(&bytes).expect("should deserialize");

    assert_eq!(restored.avatar.as_ref().map(|a| a.len()), Some(10 * 1024));
    assert_eq!(restored.avatar, Some(large_avatar));
    assert!(
        restored.verify_crc16(),
        "CRC16 must verify for large avatar"
    );
}
