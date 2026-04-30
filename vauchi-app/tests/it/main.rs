// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consolidated integration test binary for vauchi-app.

mod activity_log_engine_tests;
mod activity_log_writer_tests;
mod affected_screens_tests;
mod app_engine_activity_log_tests;
mod app_engine_navigation_tests;
mod avatar_editor_tests;
mod avatar_editor_wiring_tests;
mod backup_recovery_confirm_replace_tests;
mod component_serialization_tests;
mod contact_detail_engine_tests;
mod contact_list_intercepts_tests;
mod contact_merge_engine_tests;
mod deep_link_consent_engine_tests;
mod device_link_bridge_tests;
mod drain_notifications_tests;
mod fingerprint_verify_engine_tests;
mod group_delete_tests;
mod help_engine_wiring_tests;
mod notification_contract_tests;
mod notification_emitter_tests;
mod notification_proptest;
mod reciprocity_confirmer_tests;
mod sync_status_engine_tests;
mod update_overlay_tests;
