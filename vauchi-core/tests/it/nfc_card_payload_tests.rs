// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::NfcCardPayload;

// @internal
#[test]
fn test_crc16_deterministic() {
    let payload = NfcCardPayload::new([0xAA; 32], "Alice".to_string(), [0xBB; 32]);
    let crc1 = payload.crc16();
    let crc2 = payload.crc16();
    assert_eq!(crc1, crc2, "CRC16 must be deterministic");
    assert_ne!(crc1, 0, "CRC16 should not be zero for non-empty data");
}

// @internal
#[test]
fn test_crc16_changes_with_data() {
    let p1 = NfcCardPayload::new([0xAA; 32], "Alice".to_string(), [0xBB; 32]);
    let p2 = NfcCardPayload::new([0xCC; 32], "Alice".to_string(), [0xBB; 32]);
    assert_ne!(
        p1.crc16(),
        p2.crc16(),
        "Different identity keys must produce different CRC"
    );
}

// @internal
#[test]
fn test_crc16_changes_with_name() {
    let p1 = NfcCardPayload::new([0xAA; 32], "Alice".to_string(), [0xBB; 32]);
    let p2 = NfcCardPayload::new([0xAA; 32], "Bob".to_string(), [0xBB; 32]);
    assert_ne!(
        p1.crc16(),
        p2.crc16(),
        "Different names must produce different CRC"
    );
}

// @internal
#[test]
fn test_serialization_roundtrip() {
    let original = NfcCardPayload::new([0x11; 32], "Charlie".to_string(), [0x22; 32]);
    let bytes = original.to_bytes().expect("serialization should succeed");
    let restored = NfcCardPayload::from_bytes(&bytes).expect("deserialization should succeed");
    assert_eq!(original.identity_key, restored.identity_key);
    assert_eq!(original.display_name, restored.display_name);
    assert_eq!(original.exchange_key, restored.exchange_key);
    assert_eq!(original.crc16, restored.crc16);
    assert!(restored.verify_crc16());
}

// @internal
#[test]
fn test_crc16_verification_detects_corruption() {
    let mut payload = NfcCardPayload::new([0x11; 32], "Charlie".to_string(), [0x22; 32]);
    assert!(payload.verify_crc16());
    payload.display_name = "Corrupted".to_string();
    assert!(
        !payload.verify_crc16(),
        "CRC must fail after field mutation"
    );
}

// @internal
#[test]
fn test_empty_display_name() {
    let payload = NfcCardPayload::new([0x00; 32], String::new(), [0x00; 32]);
    let bytes = payload.to_bytes().expect("empty name should serialize");
    let restored = NfcCardPayload::from_bytes(&bytes).expect("should deserialize");
    assert!(restored.verify_crc16());
    assert_eq!(restored.display_name, "");
}

// @internal
#[test]
fn test_unicode_display_name() {
    let payload = NfcCardPayload::new(
        [0xAA; 32],
        "Mattia Egloff \u{1F44B}".to_string(),
        [0xBB; 32],
    );
    let bytes = payload.to_bytes().expect("unicode name should serialize");
    let restored = NfcCardPayload::from_bytes(&bytes).expect("should deserialize");
    assert!(restored.verify_crc16());
    assert_eq!(restored.display_name, "Mattia Egloff \u{1F44B}");
}

// @internal
#[test]
fn test_adversarial_max_length_name() {
    let long_name = "A".repeat(500);
    let payload = NfcCardPayload::new([0xFF; 32], long_name.clone(), [0xEE; 32]);
    let bytes = payload.to_bytes().expect("long name should serialize");
    let restored = NfcCardPayload::from_bytes(&bytes).expect("should deserialize");
    assert!(restored.verify_crc16());
    assert_eq!(restored.display_name, long_name);
}

// @internal
#[test]
fn test_deserialization_rejects_truncated_bytes() {
    let payload = NfcCardPayload::new([0x11; 32], "Test".to_string(), [0x22; 32]);
    let bytes = payload.to_bytes().expect("should serialize");
    let truncated = &bytes[..bytes.len() / 2];
    NfcCardPayload::from_bytes(truncated).expect_err("expected error");
}

// @internal
#[test]
fn nfc_payload_carries_no_card_fields_only_name_and_keys() {
    // G4 ratchet: NFC (TapTap) transmits only identity / display-name / keys
    // at tap time — never card fields. Card data propagates later via the
    // relay, where group visibility already applies. If a field-bearing
    // member is ever added to NfcCardPayload, this pins it so a group filter
    // must be added too. See 2026-06-08-exchange-card-not-group-filtered.
    let payload = NfcCardPayload::new([0x01; 32], "Alice".to_string(), [0x02; 32]);
    let value = serde_json::to_value(&payload).expect("serialize");
    let obj = value.as_object().expect("payload serializes to an object");
    let mut keys: Vec<&str> = obj.keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec!["crc16", "display_name", "exchange_key", "identity_key"],
        "NfcCardPayload must carry no card-field data — only name + keys"
    );
}
