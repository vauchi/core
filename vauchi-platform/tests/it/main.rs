// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Consolidated integration test binary for vauchi-platform.

mod contact_lifecycle_tests;
mod content_tests;
mod deep_link_uri_tests;
mod device_link_listener_tests;
mod error_tests;
mod exchange_session_mobile_tests;
mod ffi_boundary_tests;
mod mobile_contact_detail_tests;
mod mobile_contact_display_tests;
mod mobile_delivery_tests;
mod mobile_ui_tests;
mod mobile_visibility_resolve_tests;
mod multistage_exchange_listener_tests;
mod multistage_persistence_regression;
mod platform_app_engine_device_link_listener_tests;
mod platform_app_engine_device_link_tests;
mod platform_app_engine_domain_command_tests;
mod platform_app_engine_emergency_broadcast_tests;
mod platform_app_engine_recovery_tests;
mod platform_app_engine_tests;
mod policies_tests;
mod validation_tests;
