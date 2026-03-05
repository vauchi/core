// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Base45 encoding/decoding (RFC 9285).
//!
//! Encodes binary data into the QR Alphanumeric character set,
//! producing ~15% smaller QR codes than base64.

use thiserror::Error;

#[derive(Debug, Error)]
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

/// Encode bytes to base45 string (RFC 9285).
pub fn encode(data: &[u8]) -> String {
    let mut result = String::with_capacity(data.len() * 3 / 2 + 1);
    for chunk in data.chunks(2) {
        if chunk.len() == 2 {
            let n = (chunk[0] as u16) * 256 + chunk[1] as u16;
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

/// Decode a base45 string back to bytes (RFC 9285).
pub fn decode(data: &str) -> Result<Vec<u8>, Base45Error> {
    let bytes = data.as_bytes();
    if bytes.is_empty() {
        return Ok(vec![]);
    }
    let mut result = Vec::with_capacity(bytes.len() * 2 / 3 + 1);
    for chunk in bytes.chunks(3) {
        if chunk.len() == 3 {
            let a = char_to_val(chunk[0])?;
            let b = char_to_val(chunk[1])?;
            let c = char_to_val(chunk[2])?;
            let n = a + b * 45 + c * 45 * 45;
            result.push((n >> 8) as u8);
            result.push((n & 0xFF) as u8);
        } else if chunk.len() == 2 {
            let a = char_to_val(chunk[0])?;
            let b = char_to_val(chunk[1])?;
            let n = a + b * 45;
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
