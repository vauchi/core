// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consolidated integration test binary for vauchi-app.

mod activity_log_engine_tests;
mod activity_log_writer_tests;
mod affected_screens_tests;
mod app_engine_activity_log_tests;
mod app_engine_navigation_tests;
mod app_engine_onboarding_completion_tests;
mod ble_handshake_app_engine_tests;
// The stateful proptest needs FakeClock + DeterministicRng, both
// behind `vauchi-core/testing`. Gated so plain `cargo clippy
// --all-targets` (no features) compiles the binary cleanly.
#[cfg(feature = "testing")]
mod app_engine_stateful_proptest;
mod avatar_editor_tests;
mod avatar_editor_wiring_tests;
mod backup_recovery_confirm_replace_tests;
mod ble_handshake_machine_tests;
mod canonical_screen_id_tests;
mod component_serialization_tests;
mod contact_detail_engine_tests;
mod contact_list_intercepts_tests;
mod contact_merge_engine_tests;
mod deep_link_consent_engine_tests;
mod device_link_bridge_tests;
mod display_hint_tests;
mod drain_notifications_tests;
mod exchange_ble_invariants_proptest;
mod exchange_step_back_tests;
mod f2_new_4_settings_nav_tests;
mod file_picker_wiring_tests;
mod fingerprint_verify_engine_tests;
mod group_delete_tests;
mod help_engine_wiring_tests;
mod humble_surface_contract_tests;
mod link_exchange_tests;
mod multi_stage_machine_proptest;
mod navigate_to_tab_tests;
mod notification_contract_tests;
mod notification_emitter_tests;
mod notification_proptest;
mod reciprocity_confirmer_tests;
mod render_context_tests;
mod settings_more_parity_tests;
mod settings_render_context_tests;
mod sync_chrome_overlay_tests;
mod sync_status_engine_tests;
mod update_overlay_tests;
mod wire_humble_keys_tests;
