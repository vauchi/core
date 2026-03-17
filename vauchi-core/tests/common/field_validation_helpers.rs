// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for field validation tests.

/// Maximum value length constant (mirrors vauchi_core::contact_card::field::MAX_VALUE_LENGTH).
/// Defined here since the field module is private without the `testing` feature.
pub const MAX_VALUE_LENGTH: usize = 1000;
