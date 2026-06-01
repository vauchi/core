// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Layer 1 ScreenModel reachability per-engine tests.
//!
//! Consumer of `vauchi_app::ui::testing`. Requires the
//! `test-support` feature — enabled via `required-features` in
//! `Cargo.toml` and wired into the `just reachability` recipe
//! (plan Task 1.4).

mod backup_recovery;
mod change_password;
mod contact_detail;
mod contact_edit;
mod contact_list;
mod contact_merge;
mod contact_visibility;
mod decoy_contacts;
mod deep_link_consent;
mod delivery_status;
mod device_linking;
mod device_management;
mod device_replacement;
mod duplicate_detection;
mod duress_pin;
mod emergency_shred;
mod exchange;
mod exchange_ble;
mod fingerprint_verify;
mod form_dialog;
mod group_detail;
mod groups;
mod link_exchange;
mod link_responder;
mod multi_stage_exchange;
mod onboarding;
mod recovery;
mod recovery_claim_review;
mod sync_status;
