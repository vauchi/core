// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Typed engine↔`AppEngine` channels.
//!
//! `EngineOutput` (engine → hub) carries the salient state a workflow
//! engine exposes at completion or interception time. It replaces the
//! `as_any` downcast reads and the stringly-typed `collected_input`
//! channel: every payload that crosses the seam is a closed-enum
//! variant, so a wrong discriminator is a compile error, not a silent
//! runtime no-op.
//!
//! Mismatch policy: when the active engine is not the one a hub site
//! expects (overlay or lock engine active while a stale async result
//! lands), `engine_output()` yields `None` or a foreign variant — hub
//! sites `tracing::warn!` and degrade exactly as a failed downcast did.
//!
//! Record: `2026-06-10-appengine-typed-engine-channel`.

use super::fingerprint_verify::VerifyAction;

/// Salient typed state an engine exposes to `AppEngine`.
///
/// One variant per engine that the hub reads; each variant is the
/// engine's full interception-relevant snapshot so a single
/// parameterless getter suffices.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum EngineOutput {
    /// Outcome of the fingerprint-verification screen.
    FingerprintVerify(VerifyAction),
}
