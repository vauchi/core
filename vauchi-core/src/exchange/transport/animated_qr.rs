// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Animated QR transport — frame-sequenced QR code exchange with CRC validation.
//!
//! Splits a payload into chunked frames displayed as a cycling QR sequence.
//! Each frame carries its own CRC32 checksum so receivers can detect corruption
//! and accept frames in any order.
//!
//! Frame wire format: `{index}/{total}/{crc32_hex_8chars}/{base64url_data}`

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use thiserror::Error;

/// Configuration for animated QR sessions.
#[derive(Debug, Clone)]
pub struct AnimatedQrConfig {
    /// Frames per second for display (advisory). Default: 10.
    pub fps: u8,
    /// Maximum raw bytes per frame chunk. Default: 400.
    pub chunk_size: usize,
    /// Extra full display cycles after all frames sent. Default: 3.
    pub cycle_padding: u8,
}

impl Default for AnimatedQrConfig {
    fn default() -> Self {
        Self {
            fps: 10,
            chunk_size: 400,
            cycle_padding: 3,
        }
    }
}

/// Progress indicator returned after processing a received frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimatedQrProgress {
    /// Some chunks still missing.
    Partial { received: usize, total: usize },
    /// All chunks received — ready to reassemble.
    Complete,
}

/// Errors specific to animated QR framing.
#[derive(Debug, Error)]
pub enum AnimatedQrError {
    #[error("malformed frame: {reason}")]
    MalformedFrame { reason: String },

    #[error("CRC mismatch: expected {expected}, got {actual}")]
    CrcMismatch { expected: String, actual: String },

    #[error("frame index {index} out of range (total {total})")]
    IndexOutOfRange { index: usize, total: usize },

    #[error("cannot reassemble: received {received}/{total} chunks")]
    IncompleteReassembly { received: usize, total: usize },

    #[error("no frames received yet")]
    NoFramesReceived,
}

/// Animated QR session supporting sender, receiver, or bidirectional modes.
pub struct AnimatedQrSession {
    /// Pre-encoded frames (sender side).
    frames: Vec<String>,
    /// Current frame index for cycling (sender side).
    cursor: usize,
    /// Received raw chunks indexed by frame number (receiver side).
    received_chunks: Vec<Option<Vec<u8>>>,
    /// Total number of expected chunks (set on first received frame).
    expected_total: Option<usize>,
    /// Count of unique chunks received so far.
    received_count: usize,
    /// Session config.
    #[allow(dead_code)]
    config: AnimatedQrConfig,
}

impl AnimatedQrSession {
    /// Create a sender session that encodes `payload` into QR frames.
    pub fn new_sender(payload: Vec<u8>, config: AnimatedQrConfig) -> Self {
        let chunks: Vec<&[u8]> = payload.chunks(config.chunk_size).collect();
        let total = chunks.len().max(1);

        let frames: Vec<String> = if payload.is_empty() {
            let b64 = URL_SAFE_NO_PAD.encode([]);
            let crc = crc32fast::hash(b64.as_bytes());
            vec![format!("0/1/{:08x}/{}", crc, b64)]
        } else {
            chunks
                .iter()
                .enumerate()
                .map(|(i, chunk)| {
                    let b64 = URL_SAFE_NO_PAD.encode(chunk);
                    let crc = crc32fast::hash(b64.as_bytes());
                    format!("{}/{}/{:08x}/{}", i, total, crc, b64)
                })
                .collect()
        };

        Self {
            frames,
            cursor: 0,
            received_chunks: Vec::new(),
            expected_total: None,
            received_count: 0,
            config,
        }
    }

    /// Create a receiver session ready to accept incoming frames.
    pub fn new_receiver(config: AnimatedQrConfig) -> Self {
        Self {
            frames: Vec::new(),
            cursor: 0,
            received_chunks: Vec::new(),
            expected_total: None,
            received_count: 0,
            config,
        }
    }

    /// Number of encoded frames (sender side).
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Get the next frame in sequence, cycling back to the start after the last.
    /// Returns `None` if no frames are available (receiver-only session).
    pub fn next_frame(&mut self) -> Option<String> {
        if self.frames.is_empty() {
            return None;
        }
        let frame = self.frames[self.cursor].clone();
        self.cursor = (self.cursor + 1) % self.frames.len();
        Some(frame)
    }

    /// Get a specific frame by index.
    pub fn frame_at(&self, index: usize) -> Result<String, AnimatedQrError> {
        self.frames
            .get(index)
            .cloned()
            .ok_or(AnimatedQrError::IndexOutOfRange {
                index,
                total: self.frames.len(),
            })
    }

    /// Process a received frame string.
    ///
    /// Validates format, verifies CRC32, stores the chunk, and ignores duplicates.
    pub fn process_frame(
        &mut self,
        frame_str: &str,
    ) -> Result<AnimatedQrProgress, AnimatedQrError> {
        let parts: Vec<&str> = frame_str.splitn(4, '/').collect();
        if parts.len() != 4 {
            return Err(AnimatedQrError::MalformedFrame {
                reason: format!("expected 4 slash-separated parts, got {}", parts.len()),
            });
        }

        let index: usize = parts[0]
            .parse()
            .map_err(|_| AnimatedQrError::MalformedFrame {
                reason: format!("invalid frame index: '{}'", parts[0]),
            })?;

        let total: usize = parts[1]
            .parse()
            .map_err(|_| AnimatedQrError::MalformedFrame {
                reason: format!("invalid frame total: '{}'", parts[1]),
            })?;

        if total == 0 {
            return Err(AnimatedQrError::MalformedFrame {
                reason: "total cannot be zero".to_string(),
            });
        }

        if index >= total {
            return Err(AnimatedQrError::IndexOutOfRange { index, total });
        }

        let crc_hex = parts[2];
        if crc_hex.len() != 8 || !crc_hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(AnimatedQrError::MalformedFrame {
                reason: format!("invalid CRC32 hex: '{}'", crc_hex),
            });
        }

        let b64_data = parts[3];

        // Verify CRC32
        let expected_crc =
            u32::from_str_radix(crc_hex, 16).map_err(|_| AnimatedQrError::MalformedFrame {
                reason: format!("cannot parse CRC32 hex: '{}'", crc_hex),
            })?;
        let actual_crc = crc32fast::hash(b64_data.as_bytes());
        if expected_crc != actual_crc {
            return Err(AnimatedQrError::CrcMismatch {
                expected: format!("{:08x}", expected_crc),
                actual: format!("{:08x}", actual_crc),
            });
        }

        // Decode base64url
        let chunk =
            URL_SAFE_NO_PAD
                .decode(b64_data)
                .map_err(|e| AnimatedQrError::MalformedFrame {
                    reason: format!("invalid base64url data: {}", e),
                })?;

        // Initialize or validate total
        match self.expected_total {
            None => {
                self.expected_total = Some(total);
                self.received_chunks = vec![None; total];
            }
            Some(prev_total) if prev_total != total => {
                return Err(AnimatedQrError::MalformedFrame {
                    reason: format!("total mismatch: previously {}, now {}", prev_total, total),
                });
            }
            _ => {}
        }

        // Store chunk (ignore duplicates)
        if self.received_chunks[index].is_none() {
            self.received_chunks[index] = Some(chunk);
            self.received_count += 1;
        }

        if self.received_count == total {
            Ok(AnimatedQrProgress::Complete)
        } else {
            Ok(AnimatedQrProgress::Partial {
                received: self.received_count,
                total,
            })
        }
    }

    /// Reassemble the full payload from received chunks.
    pub fn reassemble(&self) -> Result<Vec<u8>, AnimatedQrError> {
        let total = self
            .expected_total
            .ok_or(AnimatedQrError::NoFramesReceived)?;

        if self.received_count != total {
            return Err(AnimatedQrError::IncompleteReassembly {
                received: self.received_count,
                total,
            });
        }

        let mut payload = Vec::new();
        for (i, slot) in self.received_chunks.iter().enumerate() {
            match slot {
                Some(chunk) => payload.extend_from_slice(chunk),
                None => {
                    return Err(AnimatedQrError::IncompleteReassembly {
                        received: self.received_count,
                        total: i,
                    });
                }
            }
        }

        Ok(payload)
    }
}
