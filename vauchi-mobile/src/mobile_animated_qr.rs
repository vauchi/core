// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! UniFFI bindings for animated QR transport.
//!
//! Wraps `AnimatedQrSession` for mobile (Android/iOS) usage.

use std::sync::Mutex;
use vauchi_core::exchange::transport::animated_qr::{
    AnimatedQrConfig, AnimatedQrProgress, AnimatedQrSession,
};

/// Mobile-friendly animated QR configuration.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MobileAnimatedQrConfig {
    pub fps: u8,
    pub chunk_size: u32,
    pub cycle_padding: u8,
}

impl Default for MobileAnimatedQrConfig {
    fn default() -> Self {
        Self {
            fps: 10,
            chunk_size: 400,
            cycle_padding: 3,
        }
    }
}

impl From<MobileAnimatedQrConfig> for AnimatedQrConfig {
    fn from(m: MobileAnimatedQrConfig) -> Self {
        AnimatedQrConfig {
            fps: m.fps,
            chunk_size: m.chunk_size as usize,
            cycle_padding: m.cycle_padding,
        }
    }
}

/// Progress update for animated QR reception.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum MobileAnimatedQrProgress {
    Partial { received: u32, total: u32 },
    Complete,
}

impl From<AnimatedQrProgress> for MobileAnimatedQrProgress {
    fn from(p: AnimatedQrProgress) -> Self {
        match p {
            AnimatedQrProgress::Partial { received, total } => MobileAnimatedQrProgress::Partial {
                received: received as u32,
                total: total as u32,
            },
            AnimatedQrProgress::Complete => MobileAnimatedQrProgress::Complete,
        }
    }
}

/// Sender session — encodes data into animated QR frames.
#[derive(uniffi::Object)]
pub struct MobileAnimatedQrSender {
    session: Mutex<AnimatedQrSession>,
}

#[uniffi::export]
impl MobileAnimatedQrSender {
    #[uniffi::constructor]
    pub fn new(payload: Vec<u8>, config: MobileAnimatedQrConfig) -> Self {
        Self {
            session: Mutex::new(AnimatedQrSession::new_sender(payload, config.into())),
        }
    }

    /// Total number of frames in the sequence.
    pub fn frame_count(&self) -> u32 {
        self.session.lock().unwrap().frame_count() as u32
    }

    /// Get the next frame string (cycles automatically).
    /// Returns `None` if the session has no frames (receiver-only).
    pub fn next_frame(&self) -> Option<String> {
        self.session.lock().unwrap().next_frame()
    }

    /// Get a specific frame by index.
    pub fn frame_at(&self, index: u32) -> Result<String, MobileAnimatedQrError> {
        self.session
            .lock()
            .unwrap()
            .frame_at(index as usize)
            .map_err(|e| MobileAnimatedQrError::FrameError {
                reason: format!("{}", e),
            })
    }
}

/// Receiver session — reassembles data from scanned QR frames.
#[derive(uniffi::Object)]
pub struct MobileAnimatedQrReceiver {
    session: Mutex<AnimatedQrSession>,
}

#[uniffi::export]
impl MobileAnimatedQrReceiver {
    #[uniffi::constructor]
    pub fn new(config: MobileAnimatedQrConfig) -> Self {
        Self {
            session: Mutex::new(AnimatedQrSession::new_receiver(config.into())),
        }
    }

    /// Process a scanned QR frame string.
    pub fn process_frame(
        &self,
        frame: &str,
    ) -> Result<MobileAnimatedQrProgress, MobileAnimatedQrError> {
        self.session
            .lock()
            .unwrap()
            .process_frame(frame)
            .map(MobileAnimatedQrProgress::from)
            .map_err(|e| MobileAnimatedQrError::FrameError {
                reason: format!("{}", e),
            })
    }

    /// Reassemble the complete payload after all frames received.
    pub fn reassemble(&self) -> Result<Vec<u8>, MobileAnimatedQrError> {
        self.session.lock().unwrap().reassemble().map_err(|e| {
            MobileAnimatedQrError::ReassemblyFailed {
                reason: format!("{}", e),
            }
        })
    }
}

/// Errors from animated QR operations.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum MobileAnimatedQrError {
    #[error("frame error: {reason}")]
    FrameError { reason: String },
    #[error("reassembly failed: {reason}")]
    ReassemblyFailed { reason: String },
}
