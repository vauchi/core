// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bounded wire packing for oversized public registry control envelopes.

#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

use std::borrow::Cow;
use std::io::Write;

use flate2::{Compression, Decompress, FlushDecompress, Status, write::ZlibEncoder};

use super::{GenesisError, MAX_GENESIS_PLAINTEXT, PAYLOAD_VERSION_GENESIS};
use crate::identity::device::MAX_REGISTRY_BROADCAST_JSON_BYTES;

pub(super) const COMPRESSED_CONTROL_VERSION: u8 = 0x15;
const WRAPPER_HEADER: usize = 1 + 4 + 4;
pub(super) const MAX_CONTROL_PAYLOAD: usize = 1 + 32 + 8 + 1 + MAX_REGISTRY_BROADCAST_JSON_BYTES;
pub(super) const MAX_EXPANDED_ENVELOPE: usize =
    1 + 8 + 32 + 32 + 64 + 4 + MAX_REGISTRY_BROADCAST_JSON_BYTES + 4 + MAX_CONTROL_PAYLOAD;

pub(super) fn pack(bytes: Vec<u8>, registry_control: bool) -> Result<Vec<u8>, GenesisError> {
    if bytes.first() != Some(&PAYLOAD_VERSION_GENESIS) {
        return Err(GenesisError::InvalidFormat);
    }
    if bytes.len() <= MAX_GENESIS_PLAINTEXT {
        return Ok(bytes);
    }
    if !registry_control || bytes.len() > MAX_EXPANDED_ENVELOPE {
        return Err(GenesisError::TooLarge);
    }
    let expanded_len = u32::try_from(bytes.len()).map_err(|_| GenesisError::TooLarge)?;
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&bytes)
        .map_err(|_| GenesisError::InvalidFormat)?;
    let compressed = encoder.finish().map_err(|_| GenesisError::InvalidFormat)?;
    let wire_len = WRAPPER_HEADER
        .checked_add(compressed.len())
        .ok_or(GenesisError::TooLarge)?;
    if wire_len > MAX_GENESIS_PLAINTEXT {
        return Err(GenesisError::TooLarge);
    }
    let compressed_len = u32::try_from(compressed.len()).map_err(|_| GenesisError::TooLarge)?;
    let mut wire = Vec::with_capacity(MAX_GENESIS_PLAINTEXT);
    wire.push(COMPRESSED_CONTROL_VERSION);
    wire.extend_from_slice(&expanded_len.to_be_bytes());
    wire.extend_from_slice(&compressed_len.to_be_bytes());
    wire.extend_from_slice(&compressed);
    // Fixed full-bucket padding prevents exposing compressed size (ADR-068,
    // private#1247); private card and alert envelopes never use this codec.
    wire.resize(MAX_GENESIS_PLAINTEXT, 0);
    Ok(wire)
}

pub(super) fn unpack(wire: &[u8]) -> Result<Cow<'_, [u8]>, GenesisError> {
    if wire.len() > MAX_GENESIS_PLAINTEXT {
        return Err(GenesisError::TooLarge);
    }
    match wire.first() {
        Some(&PAYLOAD_VERSION_GENESIS) => return Ok(Cow::Borrowed(wire)),
        Some(&COMPRESSED_CONTROL_VERSION) => {}
        _ => return Err(GenesisError::InvalidFormat),
    }
    if wire.len() != MAX_GENESIS_PLAINTEXT {
        return Err(GenesisError::InvalidFormat);
    }
    let expanded_len = read_len(wire.get(1..5).ok_or(GenesisError::InvalidFormat)?)?;
    let compressed_len = read_len(wire.get(5..9).ok_or(GenesisError::InvalidFormat)?)?;
    if expanded_len > MAX_EXPANDED_ENVELOPE {
        return Err(GenesisError::TooLarge);
    }
    if expanded_len <= MAX_GENESIS_PLAINTEXT {
        return Err(GenesisError::InvalidFormat);
    }
    let end = WRAPPER_HEADER
        .checked_add(compressed_len)
        .ok_or(GenesisError::InvalidFormat)?;
    let compressed = wire
        .get(WRAPPER_HEADER..end)
        .ok_or(GenesisError::InvalidFormat)?;
    if wire
        .get(end..)
        .ok_or(GenesisError::InvalidFormat)?
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err(GenesisError::InvalidFormat);
    }
    // The untrusted advertised length never controls allocation growth.
    let mut expanded = vec![0; MAX_EXPANDED_ENVELOPE + 1];
    let mut decoder = Decompress::new(true);
    let result = decoder.decompress(compressed, &mut expanded, FlushDecompress::Finish);
    if decoder.total_out() > MAX_EXPANDED_ENVELOPE as u64 {
        return Err(GenesisError::TooLarge);
    }
    let status = result.map_err(|_| GenesisError::InvalidFormat)?;
    if status != Status::StreamEnd
        || decoder.total_in() != compressed_len as u64
        || decoder.total_out() != expanded_len as u64
    {
        return Err(GenesisError::InvalidFormat);
    }
    expanded.truncate(expanded_len);
    if expanded.first() != Some(&PAYLOAD_VERSION_GENESIS) {
        return Err(GenesisError::InvalidFormat);
    }
    Ok(Cow::Owned(expanded))
}

fn read_len(bytes: &[u8]) -> Result<usize, GenesisError> {
    let array = bytes.try_into().map_err(|_| GenesisError::InvalidFormat)?;
    usize::try_from(u32::from_be_bytes(array)).map_err(|_| GenesisError::TooLarge)
}

// INLINE_TEST_REQUIRED: the private byte-codec boundary needs malformed-wire
// coverage; test bodies remain in tests/unit/genesis_control_codec_tests.rs.
#[cfg(test)]
#[path = "../../../tests/unit/genesis_control_codec_tests.rs"]
mod tests;
