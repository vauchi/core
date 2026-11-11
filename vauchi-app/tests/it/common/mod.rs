// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Common Test Utilities
//!
//! The shared arrange helpers come from `vauchi-core-testkit` (its crate
//! docs hold the idiom-to-helper table), re-exported so the `common::`
//! paths of tests moved here from vauchi-core stay as they were; the
//! helpers that drive `AppEngine` live here (vauchi/private#1230).

#[allow(unused_imports)]
pub use vauchi_core_testkit::{
    card_update, device_sync, field_validation_helpers, fixtures, helpers, linked_peer, lost_phone,
    meeting, mock_relay, per_device, recovered_phone, signed_gateway, strategies, two_recipient,
    verifiers,
};

#[allow(dead_code)]
pub mod app_engine_helpers;
