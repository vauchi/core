// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Last-used exchange defaults, persisted so a repeat exchange skips the
//! group gate and pre-applies the prior selection.

/// Last-used exchange defaults: the groups + mode the user committed to
/// on their most recent exchange. Persisted (encrypted) in `ux_state` so
/// a repeat exchange skips the group gate and pre-applies the selection
/// (M2 S1, `2026-07-03-one-tap-exchange`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExchangeDefaults {
    /// Group ids selected on the last exchange (may reference groups
    /// deleted since — consumers filter against the live group list).
    pub group_ids: Vec<String>,
    /// The exchange mode last committed to.
    pub mode: crate::exchange::mode::ExchangeMode,
}
