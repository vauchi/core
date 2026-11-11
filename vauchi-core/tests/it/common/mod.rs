// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Common Test Utilities
//!
//! The shared arrange helpers live in `vauchi-core-testkit` (its crate docs
//! hold the idiom-to-helper table); they are re-exported here so every
//! `common::` path in this test crate stays as it was (vauchi/private#1230).
//! What depends on vauchi-app stays here until its tests move there.

#[allow(unused_imports)]
pub use vauchi_core_testkit::{
    card_update, checked_locales_dir, device_sync, field_validation_helpers, fixtures, helpers,
    linked_peer, locales_dir, lost_phone, meeting, mock_relay, per_device, recovered_phone,
    signed_gateway, strategies, two_recipient, verifiers,
};
