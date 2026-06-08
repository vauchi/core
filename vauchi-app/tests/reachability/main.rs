// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Layer 1 ScreenModel reachability per-engine tests.
//!
//! Consumer of `vauchi_app::ui::testing`. Requires the
//! `test-support` feature — enabled via `required-features` in
//! `Cargo.toml` and wired into the `just reachability` recipe
//! (plan Task 1.4).

mod activity_log;
mod archived_contacts;
mod avatar_editor;
mod backup_recovery;
mod ble_exchange;
mod change_password;
mod contact_detail;
mod contact_edit;
mod contact_limit;
mod contact_list;
mod contact_merge;
mod contact_not_found;
mod contact_visibility;
mod decoy_contacts;
mod deep_link_consent;
mod delivery_status;
mod device_linking;
mod device_management;
mod device_replacement;
mod direct_transport;
mod duplicate_detection;
mod duress_pin;
mod emergency_broadcast;
mod emergency_shred;
mod exchange;
mod fingerprint_verify;
mod form_dialog;
mod gdpr;
mod group_detail;
mod groups;
mod help;
mod link_exchange;
mod link_responder;
mod lock_screen;
mod more;
mod multi_stage_exchange;
mod my_info;
mod my_info_entry_detail;
mod nfc_exchange;
mod onboarding;
mod places;
mod recovery;
mod recovery_claim_review;
mod recovery_help;
mod settings;
mod social_graph;
mod support;
mod sync_status;
mod tag_promotion;
mod tags;
