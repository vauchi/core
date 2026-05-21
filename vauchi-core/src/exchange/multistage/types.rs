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
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub enum ProtocolState {
    #[default]
    Idle,
    Advertising,
    Discovered,
    Transferring {
        chunks_sent: u16,
        chunks_total: u16,
        chunks_received: u16,
        peer_chunks_total: u16,
    },
    Verifying,
    Confirming,
    Complete,
    /// Auto-retry of the RDYY phase after first timeout.
    /// One retry is attempted before transitioning to Failed.
    RetryReady,
    Finalized,
    Failed(String),
}

/// Audio-proximity verification state used by `MultiStageSession` when
/// the active exchange mode is `Hover`. The ultrasonic handshake fires
/// after both peers reach the QR-scanning phase; `Pending` is the
/// pre-handshake state, `Listening` covers the chirp-and-listen window,
/// `Confirmed` is the success path, and `Failed` triggers the
/// proximity-specific Failed-state ScreenModel chrome (distinct from
/// generic protocol failure).
///
/// Glance ignores this state — its engine and session never transition
/// the field, because the mode never emits `AudioEmitChallenge` /
/// `AudioListenForResponse`. Lives in `vauchi-core` so the session (the
/// protocol producer) and the engine (the renderer consumer) share a
/// single source of truth — see the design pass at
/// `_private/docs/problems/2026-04-28-multi-stage-engine-hover-ultrasonic/investigation.md`
/// Option B.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioProximityState {
    Pending,
    Listening,
    Confirmed,
    Failed,
}

/// Bitmap tracking which chunks have been received.
#[derive(Debug, Clone)]
pub struct ChunkBitmap {
    bits: Vec<u8>,
    total: u16,
}

impl ChunkBitmap {
    pub fn new(total: u16) -> Self {
        let byte_count = (total as usize).div_ceil(8);
        ChunkBitmap {
            bits: vec![0u8; byte_count],
            total,
        }
    }

    pub fn mark_received(&mut self, index: u16) {
        if index < self.total {
            self.bits[index as usize / 8] |= 1 << (index % 8);
        }
    }

    pub fn has(&self, index: u16) -> bool {
        if index >= self.total {
            return false;
        }
        self.bits[index as usize / 8] & (1 << (index % 8)) != 0
    }

    pub fn received_count(&self) -> u16 {
        (0..self.total).filter(|&i| self.has(i)).count() as u16
    }

    pub fn is_complete(&self) -> bool {
        self.received_count() == self.total
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        self.bits.clone()
    }

    pub fn from_bytes(bytes: &[u8], total: u16) -> Self {
        let byte_count = (total as usize).div_ceil(8);
        let mut bits = vec![0u8; byte_count];
        let copy_len = bits.len().min(bytes.len());
        bits[..copy_len].copy_from_slice(&bytes[..copy_len]);
        ChunkBitmap { bits, total }
    }
}
