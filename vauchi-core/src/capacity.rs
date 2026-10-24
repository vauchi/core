// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Capacity hints for buffers built from several parts.

/// The parts' lengths summed as a `Vec::with_capacity` hint. Saturates
/// instead of panicking, since a hint need not be exact; it keeps encoders
/// clear of `clippy::arithmetic_side_effects` (vauchi/private#761).
pub(crate) fn capacity(parts: &[usize]) -> usize {
    parts.iter().fold(0, |sum, part| sum.saturating_add(*part))
}
