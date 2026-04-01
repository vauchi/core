// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared protocol message types for Vauchi relay communication.
//!
//! This crate defines the message envelope and payload types used between
//! clients and the relay server. It is intentionally minimal: serde only,
//! no crypto, no storage.

pub mod escrow;
pub mod messages;
pub mod v2;
pub use messages::*;
