// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::base45;

#[test]
fn test_encode_empty() {
    assert_eq!(base45::encode(&[]), "");
}

#[test]
fn test_decode_empty() {
    assert_eq!(base45::decode("").unwrap(), Vec::<u8>::new());
}

#[test]
fn test_roundtrip_hello() {
    let input = b"Hello!!";
    let encoded = base45::encode(input);
    let decoded = base45::decode(&encoded).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn test_roundtrip_binary() {
    let input: Vec<u8> = (0..=255).collect();
    let encoded = base45::encode(&input);
    let decoded = base45::decode(&encoded).unwrap();
    assert_eq!(decoded, input);
}

#[test]
fn test_encode_uses_alphanumeric_charset() {
    let input = b"test data for QR";
    let encoded = base45::encode(input);
    let valid_chars = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";
    for ch in encoded.chars() {
        assert!(valid_chars.contains(ch), "invalid char: {ch}");
    }
}

#[test]
fn test_decode_invalid_char() {
    assert!(base45::decode("abc").is_err()); // lowercase not in charset
}

#[test]
fn test_rfc9285_vector() {
    // RFC 9285 test vector: "AB" encodes to "BB8"
    assert_eq!(base45::encode(b"AB"), "BB8");
    assert_eq!(base45::decode("BB8").unwrap(), b"AB");
}
