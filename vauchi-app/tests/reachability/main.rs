// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Layer 1 ScreenModel reachability per-engine tests.
//!
//! Consumer of `vauchi_app::ui::testing`. Requires the
//! `test-support` feature — enabled via `required-features` in
//! `Cargo.toml` and wired into the `just reachability` recipe
//! (plan Task 1.4).

mod exchange;
mod form_dialog;
mod onboarding;
mod recovery;
mod sync_status;
