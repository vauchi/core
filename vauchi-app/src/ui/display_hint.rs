// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Form-factor hint passed at engine construction (ADR-023).
//!
//! Engines that compose screens differently for phones, watches, or
//! desktops take a [`DisplayHint`] at `new()`. The hint may cross
//! engine-construction binding boundaries (UniFFI / CABI / serde
//! configs); what it must NOT do is appear on `ScreenModel`. Per
//! ADR-021/043, frontends never branch on form factor — they pass
//! the hint at construction once, then forget it.
//!
//! Engines that don't branch on form factor can ignore the hint and
//! omit the parameter entirely. The
//! `.claude/rules/adr-constraints.md` rule that "every
//! `WorkflowEngine` impl handles all variants" applies to engines
//! that do take it: each branch must produce a sensible
//! `ScreenModel` for [`DisplayHint::Phone`], [`DisplayHint::Watch`],
//! and [`DisplayHint::Desktop`].

use serde::{Deserialize, Serialize};

/// Form factor of the rendering frontend, per `ADR-023`.
///
/// Marked `#[non_exhaustive]` so additional variants (e.g. `Tv`,
/// `CarPlay`) can be added without breaking out-of-tree consumers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum DisplayHint {
    Phone,
    Watch,
    Desktop,
}
