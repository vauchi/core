// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Chunking
//!
//! Splits encrypted payloads into BLE MTU-sized chunks and reassembles
//! them on the receiving side.
//!
//! Chunk format: `[chunk_index: u16 LE][total_chunks: u16 LE][data]`

use super::error::ExchangeError;

/// Overhead per chunk: 2 bytes chunk_index + 2 bytes total_chunks.
pub const BLE_CHUNK_OVERHEAD: usize = 4;

/// Splits a byte buffer into BLE MTU-sized chunks.
pub struct BleChunker {
    _private: (),
}

impl BleChunker {
    /// Create a new chunker. `mtu_usable` is the MTU minus the ATT header (3 bytes).
    pub fn new(_data: &[u8], _mtu_usable: usize) -> Self {
        todo!()
    }

    /// Total number of chunks needed.
    pub fn total_chunks(&self) -> u16 {
        todo!()
    }

    /// Returns the full chunk packet (4-byte header + data) for the given index.
    pub fn chunk(&self, _index: u16) -> Option<Vec<u8>> {
        todo!()
    }
}

/// Reassembles BLE chunks into the original payload.
pub struct BleReassembler {
    _private: (),
}

impl BleReassembler {
    /// Create a new reassembler expecting `total` chunks.
    pub fn new(_total: u16) -> Self {
        todo!()
    }

    /// Parse and insert a chunk packet.
    pub fn insert_chunk(&mut self, _packet: &[u8]) -> Result<(), ExchangeError> {
        todo!()
    }

    /// Whether all chunks have been received.
    pub fn is_complete(&self) -> bool {
        todo!()
    }

    /// Number of unique chunks received so far.
    pub fn received_count(&self) -> u16 {
        todo!()
    }

    /// Assemble the original payload. Returns `None` if not complete.
    pub fn assemble(&self) -> Option<Vec<u8>> {
        todo!()
    }
}
