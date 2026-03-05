// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CRC-16/CCITT-FALSE checksum for per-chunk integrity in DATA stage QR codes.
//!
//! Fast, 2-byte checksum sufficient for detecting scan errors.
//! Polynomial: 0x1021, Init: 0xFFFF, no final XOR.

/// Compute CRC-16/CCITT-FALSE over data.
/// Polynomial: 0x1021, Init: 0xFFFF, no final XOR.
pub fn compute(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= (byte as u16) << 8;
        for _ in 0..8 {
            if crc & 0x8000 != 0 {
                crc = (crc << 1) ^ 0x1021;
            } else {
                crc <<= 1;
            }
        }
    }
    crc
}

/// Verify that data matches expected CRC16.
#[allow(dead_code)]
pub fn verify(data: &[u8], expected: u16) -> bool {
    compute(data) == expected
}
