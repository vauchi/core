// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-stage exchange protocol types.
//!
//! Foundation types for the 5-stage atomic QR exchange:
//! - [`ProtocolState`] — state machine enum
//! - [`ChunkBitmap`] — tracks chunk receipt during DATA stage
//! - [`QrPayload`] — what core returns to mobile apps for QR display

/// QR payload returned to the mobile app for display.
#[derive(Debug, Clone)]
pub struct QrPayload {
    /// Raw string to encode into a QR code.
    pub data: String,
    /// Suggested error correction level: "L" or "M".
    pub error_correction: String,
    /// Suggested minimum display duration in milliseconds.
    pub display_duration_ms: u32,
}

/// Protocol state machine states.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolState {
    Idle,
    Advertising,
    Discovered,
    Transferring {
        chunks_sent: u8,
        chunks_total: u8,
        chunks_received: u8,
        peer_chunks_total: u8,
    },
    Verifying,
    Confirming,
    Complete,
    Failed(String),
}

impl Default for ProtocolState {
    fn default() -> Self {
        ProtocolState::Idle
    }
}

/// Bitmap tracking which chunks have been received.
#[derive(Debug, Clone)]
pub struct ChunkBitmap {
    bits: Vec<u8>,
    total: u8,
}

impl ChunkBitmap {
    pub fn new(total: u8) -> Self {
        let byte_count = ((total as usize) + 7) / 8;
        ChunkBitmap {
            bits: vec![0u8; byte_count],
            total,
        }
    }

    pub fn total(&self) -> u8 {
        self.total
    }

    pub fn mark_received(&mut self, index: u8) {
        if index < self.total {
            self.bits[index as usize / 8] |= 1 << (index % 8);
        }
    }

    pub fn has(&self, index: u8) -> bool {
        if index >= self.total {
            return false;
        }
        self.bits[index as usize / 8] & (1 << (index % 8)) != 0
    }

    pub fn received_count(&self) -> u8 {
        (0..self.total).filter(|&i| self.has(i)).count() as u8
    }

    pub fn is_complete(&self) -> bool {
        self.received_count() == self.total
    }

    /// Next un-received chunk index, or `None` if all received.
    pub fn next_missing(&self) -> Option<u8> {
        (0..self.total).find(|&i| !self.has(i))
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bits.clone()
    }

    pub fn from_bytes(bytes: &[u8], total: u8) -> Self {
        let byte_count = ((total as usize) + 7) / 8;
        let mut bits = vec![0u8; byte_count];
        let copy_len = bits.len().min(bytes.len());
        bits[..copy_len].copy_from_slice(&bytes[..copy_len]);
        ChunkBitmap { bits, total }
    }
}
