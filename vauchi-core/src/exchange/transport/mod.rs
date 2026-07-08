// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Transport types and utilities for exchange protocols.
//!
//! Animated QR and the X25519+XChaCha20-Poly1305 transport protocol.
//! Hardware I/O uses the `Command`/`Event` protocol (ADR-031).

pub mod animated_qr;
pub mod protocol;
