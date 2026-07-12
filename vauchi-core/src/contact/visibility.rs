// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Visibility Rules for Contact Fields
//!
//! The types + logic now live in the neutral `crate::visibility` module
//! (kept out of both `contact` and `contact_card` to avoid a cycle); this
//! re-export preserves the `contact::visibility` path for existing callers.

pub use crate::visibility::{FieldVisibility, VisibilityRules};
