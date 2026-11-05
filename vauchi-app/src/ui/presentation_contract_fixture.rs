// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Core-owned product journey, validated against the pinned shell contract.
//!
//! Apps may select these bytes as compatibility evidence for their assembly.
//! Standalone shell conformance uses contract-owned examples instead.
//! The matching integration test replays each recorded [`vauchi_core::Event`]
//! through [`super::AppEngine`] and ratchets the complete ordered
//! [`vauchi_core::Command`] batches.

/// Return the version-1 presentation contract fixture as canonical JSON.
pub fn presentation_contract_fixture_json() -> &'static str {
    include_str!("../../fixtures/journeys/presentation_contract_v1.json")
}
