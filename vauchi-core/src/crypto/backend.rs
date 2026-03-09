// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Crypto backend re-exports.
//!
//! Default: aws-lc-rs (FIPS 140-3 Level 1 certified, backed by AWS)
//! WASM: RustCrypto (audited, pure Rust) — enabled via `crypto-wasm` feature

// For now, both ring and aws-lc-rs coexist during migration.
// After migration is complete (Task 8), ring will be removed.
