// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-Stage Atomic QR Exchange Protocol
//!
//! 5-stage protocol: INIT → DATA → VERIFY → CONFIRM → COMPLETE
//! Transfers full contact cards face-to-face via chunked QR codes
//! with atomic commitment scheme.

pub mod base45;
pub mod chunker;
pub mod commitment;
pub mod crc16;
pub mod qr_codec;
pub mod session;
pub mod types;
