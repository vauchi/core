// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consolidated integration test binary for vauchi-app.

mod accessibility_tokens_tests;
mod activity_log_engine_tests;
mod activity_log_writer_tests;
mod affected_screens_tests;
mod app_engine_activity_log_tests;
mod app_engine_add_field_group_grant_tests;
mod app_engine_add_field_validation_tests;
mod app_engine_hardware_event_copy_tests;
mod app_engine_invalidation_tests;
mod app_engine_navigation_tests;
mod app_engine_onboarding_completion_tests;
mod app_lifecycle_tests;
#[path = "../app_reducer_protocol_tests.rs"]
mod app_reducer_protocol_tests;
#[path = "../app_undo_protocol_tests.rs"]
mod app_undo_protocol_tests;
mod ble_handshake_app_engine_tests;
mod schedule_wakeup_tests;
// The multi-stage half needs FakeClock (gated inside the file); the
// BLE half runs featureless.
mod ceremony_wiring_tests;
// The stateful proptest needs FakeClock + DeterministicRng, both
// behind `vauchi-core/testing`. Gated so plain `cargo clippy
// --all-targets` (no features) compiles the binary cleanly.
#[cfg(feature = "testing")]
mod app_engine_stateful_proptest;
mod avatar_editor_tests;
mod avatar_editor_wiring_tests;
mod avatar_i18n_tests;
mod backup_recovery_confirm_replace_tests;
mod backup_reminder_toast_tests;
mod batch2_i18n_tests;
mod batch3_i18n_tests;
mod ble_exchange_app_engine_tests;
mod ble_handshake_machine_tests;
mod ble_pair_fault_gate_tests;
mod canonical_screen_id_tests;
mod component_serialization_tests;
mod contact_detail_engine_tests;
mod contact_detail_i18n_tests;
mod contact_detail_intercepts_tests;
mod contact_detail_place_tests;
mod contact_list_faceted_tests;
mod contact_list_i18n_tests;
mod contact_list_intercepts_tests;
mod contact_merge_engine_tests;
#[path = "../contextual_surface_tests.rs"]
mod contextual_surface_tests;
#[path = "../contextual_undo_tests.rs"]
mod contextual_undo_tests;
mod deep_link_consent_engine_tests;
mod device_link_bridge_tests;
mod device_link_two_machine_tests;
mod device_linking_i18n_tests;
mod device_linking_receiver_i18n_tests;
mod device_management_i18n_tests;
mod device_replacement_i18n_tests;
mod direct_transport_app_engine_tests;
mod display_hint_tests;
mod drain_notifications_tests;
mod duress_backup_i18n_tests;
mod duress_pin_wiring_tests;
mod engine_output_tests;
mod engine_update_tests;
mod exchange_ble_chrome_i18n_tests;
mod exchange_ble_invariants_proptest;
mod exchange_ble_rich_success_tests;
mod exchange_cancel_navigation_tests;
mod exchange_flow_i18n_tests;
mod exchange_group_filter_preview_tests;
mod exchange_group_selection_actions_tests;
mod exchange_last_used_defaults_tests;
mod exchange_location_capture_tests;
mod exchange_no_numeric_progress_tests;
mod exchange_picker_hero_tests;
mod exchange_picker_i18n_tests;
mod exchange_step_back_tests;
mod f2_new_4_settings_nav_tests;
mod field_visibility_label_tests;
mod file_picker_wiring_tests;
mod fingerprint_verify_engine_tests;
mod form_dialog_i18n_tests;
mod gdpr_i18n_tests;
mod group_delete_tests;
mod help_engine_wiring_tests;
mod humble_surface_contract_tests;
mod i18n_support;
mod inline_confirm_action_id_tests;
mod last_pins_i18n_tests;
mod legacy_projection_matrix_tests;
mod link_exchange_i18n_tests;
mod link_exchange_tests;
mod local_rendezvous_tests;
mod locale_provenance_tests;
mod more_i18n_tests;
mod multi_stage_deadline_tests;
mod multi_stage_exchange_i18n_tests;
mod multi_stage_machine_proptest;
mod nfc_exchange_app_engine_tests;
mod places_tests;
#[path = "../prepared_surface_tests.rs"]
mod prepared_surface_tests;
mod tag_promotion_tests;
mod tags_engine_tests;
mod tags_intercepts_tests;
mod tags_list_i18n_tests;
mod transport_readiness_wiring_tests;
// Needs `FakeClock`, behind `vauchi-core/testing`. Gated so plain
// `cargo clippy --all-targets` (no testing feature) still compiles.
mod context_bar_overlay_toggle_tests;
mod lock_screen_navigation_tests;
#[cfg(feature = "testing")]
mod multi_stage_persist_reciprocity_tests;
#[cfg(feature = "testing")]
mod multi_stage_poll_cadence_tests;
mod multi_stage_presentation_scan_tests;
mod multi_stage_two_party_tests;
mod nav_chrome_overlay_tests;
mod nav_destination_self_reference_tests;
mod nav_tab_id_tests;
mod navigate_back_action_tests;
mod navigate_to_tab_tests;
mod notification_contract_tests;
mod notification_emitter_tests;
mod notification_proptest;
mod onboarding_custom_group_tests;
mod onboarding_i18n_tests;
mod reciprocity_confirmer_tests;
mod recovery_claim_review_i18n_tests;
mod recovery_help_i18n_tests;
mod recovery_status_i18n_tests;
mod render_context_tests;
#[path = "../responsive_presentation_tests.rs"]
mod responsive_presentation_tests;
mod result_routing_wiring_tests;
mod settings_link_routing_tests;
mod settings_more_parity_tests;
mod settings_profile_i18n_tests;
mod settings_render_context_tests;
mod settings_row_control_label_tests;
mod settings_security_i18n_tests;
mod shred_i18n_tests;
mod status_indicator_label_tests;
mod sync_chrome_overlay_tests;
mod update_overlay_tests;
mod wire_humble_keys_tests;
