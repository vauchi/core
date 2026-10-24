// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Base45 encoding/decoding (RFC 9285).
//!
//! Encodes binary data into the QR Alphanumeric character set,
//! producing ~15% smaller QR codes than base64.

#![cfg_attr(not(test), deny(clippy::arithmetic_side_effects))]

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Base45Error {
    #[error("invalid base45 character: {0}")]
    InvalidCharacter(char),
    #[error("invalid base45 length")]
    InvalidLength,
    #[error("value overflow in base45 decoding")]
    Overflow,
}

const CHARSET: &[u8; 45] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ $%*+-./:";

fn char_to_val(c: u8) -> Result<u32, Base45Error> {
    CHARSET
        .iter()
        .position(|&ch| ch == c)
        .map(|p| p as u32)
        .ok_or(Base45Error::InvalidCharacter(c as char))
}

/// The character for one base45 digit.
///
/// # Panics
///
/// Panics if `value` is 45 or more.
pub fn digit(value: u8) -> char {
    CHARSET[usize::from(value)] as char
}

/// The value of one base45 digit, 0–44.
pub fn digit_value(c: u8) -> Result<u8, Base45Error> {
    char_to_val(c).map(|v| v as u8)
}

/// Encode bytes to base45 string (RFC 9285).
pub fn encode(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len().div_ceil(2).saturating_mul(3));
    for chunk in data.chunks(2) {
        if chunk.len() == 2 {
            let n = u16::from_be_bytes([chunk[0], chunk[1]]);
            let c = n / (45 * 45);
            let remainder = n % (45 * 45);
            let b = remainder / 45;
            let a = remainder % 45;
            result.push(CHARSET[a as usize] as char);
            result.push(CHARSET[b as usize] as char);
            result.push(CHARSET[c as usize] as char);
        } else {
            let n = chunk[0] as u16;
            let b = n / 45;
            let a = n % 45;
            result.push(CHARSET[a as usize] as char);
            result.push(CHARSET[b as usize] as char);
        }
    }
    result
}

/// The value of one chunk's digits, least significant first.
// INVARIANT: every digit is below 45, so three digits stay under
// 45^3 = 91_125 and no step can overflow a u32 (vauchi/private#761).
#[allow(clippy::arithmetic_side_effects)]
fn chunk_value(digits: &[u32]) -> u32 {
    digits
        .iter()
        .rev()
        .fold(0, |value, digit| value * 45 + digit)
}

/// Decode a base45 string back to bytes (RFC 9285).
pub fn decode(data: &str) -> Result<Vec<u8>, Base45Error> {
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    let mut result = Vec::with_capacity(bytes.len().div_ceil(3).saturating_mul(2));
    for chunk in bytes.chunks(3) {
        if chunk.len() == 3 {
            let a = char_to_val(chunk[0])?;
            let b = char_to_val(chunk[1])?;
            let c = char_to_val(chunk[2])?;
            let n = chunk_value(&[a, b, c]);
            if n > 65535 {
                return Err(Base45Error::Overflow);
            }
            result.push((n >> 8) as u8);
            result.push((n & 0xFF) as u8);
        } else if chunk.len() == 2 {
            let a = char_to_val(chunk[0])?;
            let b = char_to_val(chunk[1])?;
            let n = chunk_value(&[a, b]);
            if n > 255 {
                return Err(Base45Error::Overflow);
            }
            result.push(n as u8);
        } else {
            return Err(Base45Error::InvalidLength);
        }
    }
    Ok(result)
}
