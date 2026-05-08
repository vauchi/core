// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Form-factor hint passed at engine construction (ADR-023).
//!
//! Engines that compose screens differently for phones, watches, or
//! desktops take a [`DisplayHint`] at `new()`. The hint is internal
//! to core's screen composition — it never appears on `ScreenModel`
//! and never crosses the serde / UniFFI / CABI boundary, so
//! frontends never branch on form factor.
//!
//! Engines that don't branch on form factor can ignore the hint and
//! omit the parameter entirely. The
//! `.claude/rules/adr-constraints.md` rule that "every
//! `WorkflowEngine` impl handles all variants" applies to engines
//! that do take it: each branch must produce a sensible
//! `ScreenModel` for [`DisplayHint::Phone`], [`DisplayHint::Watch`],
//! and [`DisplayHint::Desktop`].

/// Form factor of the rendering frontend, per `ADR-023`.
///
/// Deliberately *not* `Serialize`/`Deserialize` — this type must
/// not cross the wire. Frontends learn the form factor from the OS,
/// not from a `ScreenModel` field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisplayHint {
    Phone,
    Watch,
    Desktop,
}
