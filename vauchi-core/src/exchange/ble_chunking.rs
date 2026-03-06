// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! BLE Chunking
//!
//! Splits encrypted payloads into BLE MTU-sized chunks and reassembles
//! them on the receiving side.
//!
//! Chunk format: `[chunk_index: u16 LE][total_chunks: u16 LE][data]`

use std::collections::HashMap;

use super::error::ExchangeError;

/// Overhead per chunk: 2 bytes chunk_index + 2 bytes total_chunks.
pub const BLE_CHUNK_OVERHEAD: usize = 4;

/// Splits a byte buffer into BLE MTU-sized chunks.
pub struct BleChunker {
    data: Vec<u8>,
    chunk_data_size: usize,
    total: u16,
}

impl BleChunker {
    /// Create a new chunker. `mtu_usable` is the MTU minus the ATT header (3 bytes).
    pub fn new(data: &[u8], mtu_usable: usize) -> Self {
        let chunk_data_size = mtu_usable.saturating_sub(BLE_CHUNK_OVERHEAD).max(1);
        let total = if data.is_empty() {
            1
        } else {
            data.len().div_ceil(chunk_data_size) as u16
        };
        Self {
            data: data.to_vec(),
            chunk_data_size,
            total,
        }
    }

    /// Total number of chunks needed.
    pub fn total_chunks(&self) -> u16 {
        self.total
    }

    /// Returns the full chunk packet (4-byte header + data) for the given index.
    pub fn chunk(&self, index: u16) -> Option<Vec<u8>> {
        if index >= self.total {
            return None;
        }

        let start = index as usize * self.chunk_data_size;
        let end = (start + self.chunk_data_size).min(self.data.len());
        let slice = &self.data[start..end];

        let mut packet = Vec::with_capacity(BLE_CHUNK_OVERHEAD + slice.len());
        packet.extend_from_slice(&index.to_le_bytes());
        packet.extend_from_slice(&self.total.to_le_bytes());
        packet.extend_from_slice(slice);
        Some(packet)
    }
}

/// Reassembles BLE chunks into the original payload.
pub struct BleReassembler {
    total: u16,
    chunks: HashMap<u16, Vec<u8>>,
}

impl BleReassembler {
    /// Create a new reassembler expecting `total` chunks.
    pub fn new(total: u16) -> Self {
        Self {
            total,
            chunks: HashMap::with_capacity(total as usize),
        }
    }

    /// Parse and insert a chunk packet.
    pub fn insert_chunk(&mut self, packet: &[u8]) -> Result<(), ExchangeError> {
        if packet.len() < BLE_CHUNK_OVERHEAD {
            return Err(ExchangeError::BleChunkReassemblyFailed(
                "packet too short".into(),
            ));
        }

        let index = u16::from_le_bytes([packet[0], packet[1]]);
        let total = u16::from_le_bytes([packet[2], packet[3]]);

        if total != self.total {
            return Err(ExchangeError::BleChunkReassemblyFailed(format!(
                "total mismatch: expected {}, got {total}",
                self.total
            )));
        }

        if index >= self.total {
            return Err(ExchangeError::BleChunkReassemblyFailed(format!(
                "index {index} out of range (total {total})"
            )));
        }

        let data = packet[BLE_CHUNK_OVERHEAD..].to_vec();
        // Idempotent: duplicate inserts are silently accepted
        self.chunks.entry(index).or_insert(data);

        Ok(())
    }

    /// Whether all chunks have been received.
    pub fn is_complete(&self) -> bool {
        self.chunks.len() == self.total as usize
    }

    /// Number of unique chunks received so far.
    pub fn received_count(&self) -> u16 {
        self.chunks.len() as u16
    }

    /// Assemble the original payload. Returns `None` if not complete.
    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }

        let mut result = Vec::new();
        for i in 0..self.total {
            result.extend_from_slice(self.chunks.get(&i)?);
        }
        Some(result)
    }
}
