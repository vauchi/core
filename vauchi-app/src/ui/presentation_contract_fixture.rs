// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Versioned, Core-owned presentation contract corpus.
//!
//! Every shell consumes these exact bytes through its normal Core binding.
//! The matching integration test replays each recorded [`vauchi_core::Event`]
//! through [`super::AppEngine`] and ratchets the complete ordered
//! [`vauchi_core::Command`] batches.

/// Return the version-1 presentation contract fixture as canonical JSON.
pub fn presentation_contract_fixture_json() -> &'static str {
    include_str!("../../fixtures/presentation_contract_v1.json")
}
