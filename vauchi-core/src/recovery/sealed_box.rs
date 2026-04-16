// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Sealed-box encryption for guardian token relay storage.
//!
//! Implements anonymous sender encryption using ephemeral X25519 key agreement
//! and XChaCha20-Poly1305 AEAD. Only the designated recipient (guardian) can
//! decrypt their entry. The sender is anonymous — no long-term sender key is
//! included in the output.
//!
//! # Output format
//!
//! `ephemeral_pk (32) || nonce (24) || ciphertext+tag`
//!
//! Minimum sealed size for `open`: 32 + 24 + 16 = 72 bytes.
//!
//! # Key derivation
//!
//! SHA-256("vauchi-sealed-box-v1" || shared_secret) → 32-byte symmetric key.
