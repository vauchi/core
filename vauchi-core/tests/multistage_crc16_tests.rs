// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

use vauchi_core::exchange::multistage::crc16;

#[test]
fn test_crc16_empty() {
    assert_eq!(crc16::compute(&[]), 0xFFFF); // CRC-CCITT init value
}

#[test]
fn test_crc16_known_vector() {
    // "123456789" -> CRC-CCITT = 0x29B1
    assert_eq!(crc16::compute(b"123456789"), 0x29B1);
}

#[test]
fn test_crc16_verify_pass() {
    let data = b"hello world";
    let checksum = crc16::compute(data);
    assert!(crc16::verify(data, checksum));
}

#[test]
fn test_crc16_verify_fail() {
    let data = b"hello world";
    assert!(!crc16::verify(data, 0x0000));
}

#[test]
fn test_crc16_different_data_different_checksum() {
    let crc_a = crc16::compute(b"alice");
    let crc_b = crc16::compute(b"bob");
    assert_ne!(crc_a, crc_b);
}
