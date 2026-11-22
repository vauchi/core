// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Random bytes from the operating system's generator.

use rand_core::RngCore;

/// Generate cryptographically secure random bytes.
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    rand_core::OsRng.fill_bytes(&mut buf);
    buf
}

/// Fill a mutable slice with cryptographically secure random bytes.
pub fn random_fill(buf: &mut [u8]) {
    rand_core::OsRng.fill_bytes(buf);
}
