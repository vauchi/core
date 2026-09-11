// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Versioned, Core-owned screen catalog: one generic presentation command
//! batch per reachable screen, rendered from seeded sample data.
//!
//! Every shell is a humble renderer of [`vauchi_core::Command`]s (ADR-066),
//! so replaying one entry's `commands` through a shell's real renderer
//! draws that screen headlessly — no hardware, peer, or relay needed. The
//! integration test `tests/screen_catalog_fixture_tests.rs` builds the
//! catalog from an in-memory [`super::AppEngine`] seeded with an identity,
//! contacts, groups, and own-card entries, and pins the checked-in file's
//! structure (code ids, surface titles, node kinds) against a fresh build.
//!
//! Keys and ids are random per run, so the bytes are not byte-stable; the
//! `#[ignore]` `regenerate_shared_fixture` test rewrites the file on demand.
//!
//! Screens that are not in the catalog, and why:
//! - `DeepLinkConsent` / `DeepLinkResponder` / `DeviceLinkJoin` need a
//!   parsed peer payload or invitation URL that only a real peer emits.
//! - `TagPromotion` needs a tag id; tags are created only through the
//!   Tags screen flow, which the catalog does not drive.
//! - Exchange screens are captured in their initial state only: their
//!   in-progress, stalled, and terminal states are reached through
//!   hardware events (`BleDeviceDiscovered`, `QrScanned`, `NfcApduReceived`,
//!   relay escrow) with no deterministic in-memory sequence.

use serde::{Deserialize, Serialize};
use vauchi_core::Command;

/// The version-1 screen catalog: every reachable screen, one batch each.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenCatalogFixture {
    pub schema_version: u32,
    pub screens: Vec<ScreenCatalogEntry>,
}

/// One screen in one locale: the exact command batch the engine emits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenCatalogEntry {
    /// The screen's [`super::AppScreen::screen_id`], suffixed with
    /// `-<variant>` when the same screen is captured in several states
    /// (for example `contacts-empty` next to `contacts`).
    pub code_id: String,
    /// The rendered surface title, duplicated here for quick lookup.
    pub title: String,
    /// The render locale code the batch was produced under (`en`, `de`).
    pub locale: String,
    /// The ordered batch `AppEngine::initial_commands` returned.
    pub commands: Vec<Command>,
}

/// Return the version-1 screen catalog fixture as canonical JSON.
pub fn screen_catalog_fixture_json() -> &'static str {
    include_str!("../../fixtures/screen_catalog_v1.json")
}
