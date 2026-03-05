// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Chunking engine for multi-QR data transfer.
//!
//! Splits a byte payload into fixed-size chunks that fit in a single QR code,
//! and reassembles chunks received out of order.

/// Splits a byte payload into fixed-size chunks for QR encoding.
pub struct Chunker<'a> {
    data: &'a [u8],
    chunk_size: usize,
}

impl<'a> Chunker<'a> {
    pub fn new(data: &'a [u8], max_chunk_bytes: usize) -> Self {
        Chunker {
            data,
            chunk_size: max_chunk_bytes,
        }
    }

    pub fn total_chunks(&self) -> u8 {
        if self.data.is_empty() {
            return 1;
        }
        ((self.data.len() + self.chunk_size - 1) / self.chunk_size) as u8
    }

    pub fn chunk(&self, index: u8) -> Option<&[u8]> {
        let start = index as usize * self.chunk_size;
        if start >= self.data.len() && index > 0 {
            return None;
        }
        let end = (start + self.chunk_size).min(self.data.len());
        Some(&self.data[start..end])
    }
}

/// Reassembles chunks received out of order.
pub struct ReassemblyBuffer {
    chunks: Vec<Option<Vec<u8>>>,
    received: u8,
    total: u8,
}

impl ReassemblyBuffer {
    pub fn new(total: u8) -> Self {
        ReassemblyBuffer {
            chunks: vec![None; total as usize],
            received: 0,
            total,
        }
    }

    pub fn insert(&mut self, index: u8, data: Vec<u8>) {
        if index < self.total && self.chunks[index as usize].is_none() {
            self.chunks[index as usize] = Some(data);
            self.received += 1;
        }
    }

    pub fn has(&self, index: u8) -> bool {
        index < self.total && self.chunks[index as usize].is_some()
    }

    pub fn is_complete(&self) -> bool {
        self.received == self.total
    }

    pub fn received_count(&self) -> u8 {
        self.received
    }

    /// Assemble all chunks in order. Returns None if incomplete.
    pub fn assemble(&self) -> Option<Vec<u8>> {
        if !self.is_complete() {
            return None;
        }
        let mut result = Vec::new();
        for chunk in &self.chunks {
            result.extend_from_slice(chunk.as_ref()?);
        }
        Some(result)
    }
}
