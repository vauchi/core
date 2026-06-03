// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `PlatformAppEngineTestHelpers` — integration-test scaffolding for
//! `PlatformAppEngine`.
//!
//! Lives in its own module (extracted from `platform_app_engine.rs`
//! 2026-05-20) so the binding-facing PAE file stays the canonical home
//! for production surface, and so the file-size baseline for that file
//! doesn't grow with test-helper additions. Trait + impl are both
//! `#[doc(hidden)]` — invisible to the UniFFI binding generator (no
//! per-method `#[uniffi::export]` on trait impls) and invisible to the
//! humble-surface ratchet's source-text scanner (`impl … for
//! PlatformAppEngine {` blocks fall outside the
//! `^impl PlatformAppEngine {` regex).
//!
//! See:
//! - `_private/docs/problems/2026-05-20-pae-test-helper-containment/`
//! - `core/vauchi-app/tests/it/humble_surface_contract_tests.rs`
//!   (`SURPLUS_RATCHET_CEILING` rationale)

use crate::error::MobileError;
use crate::platform_app_engine::PlatformAppEngine;

/// Test-only helpers on `PlatformAppEngine`.
///
/// Exposed as a `#[doc(hidden)]` trait rather than `pub fn`s on
/// `PlatformAppEngine` itself so the humble-surface contract test
/// (`humble_surface_contract_tests::platform_app_engine_surface_respects_ratchet`)
/// does not classify them as binding surplus. The contract test scans
/// for `impl PlatformAppEngine {`; trait `impl … for PlatformAppEngine {`
/// blocks fall outside that match, which matches the architectural
/// intent: these are integration-test scaffolding, not the humble
/// surface frontends route through.
#[doc(hidden)]
pub trait PlatformAppEngineTestHelpers {
    /// Save a contact directly to storage.
    ///
    /// Used by integration tests that need exchanged or imported
    /// contacts without running a full exchange flow or VCF import.
    /// Mirrors `VauchiPlatform::save_test_contact` (slice 32g retires
    /// the `VauchiPlatform` copy).
    fn save_test_contact(&self, contact: &vauchi_core::Contact) -> Result<(), MobileError>;

    /// Save a delivery record directly to storage.
    ///
    /// Used by integration tests that need delivery records of specific
    /// statuses without running the full sync/delivery pipeline.
    /// Mirrors the legacy `VauchiPlatform::save_test_delivery_record`
    /// (retired in the 2026-05-18 Phase 2a get-failed-delivery-relocate slice).
    fn save_test_delivery_record(
        &self,
        record: &vauchi_core::storage::DeliveryRecord,
    ) -> Result<(), MobileError>;

    // ── Multi-stage exchange test helpers retired (slice 32m T1.3) ──
    //
    // The 4 `apply_multi_stage_*_for_test` methods were scaffolding
    // around the pre-32m cycle-thread race (each first called
    // `cancel_multi_stage_session()` to stop the cycle thread from
    // re-pushing `Idle` over a test's manual state push). Slice 32m
    // T1.2c retired the cycle thread; the race no longer exists. The
    // new MultiStageMachine proptest
    // (`vauchi-app/tests/it/multi_stage_machine_proptest.rs`) +
    // reachability tests
    // (`vauchi-app/tests/reachability/multi_stage_exchange.rs`) cover
    // the invariants these fixtures asserted on the engine's
    // rendering side.

    // ── Device-link test helpers ───────────────────────────────────
    //
    // Moved out of the bare `impl PlatformAppEngine` block in
    // `platform_app_engine_device_link.rs` (2026-05-20) for the same
    // ratchet-pollution reason as the multi-stage methods above.
    // These never shipped in the UniFFI binding (the source block
    // was not `#[uniffi::export]`-tagged) so this move is metric-
    // hygiene only on the binding side.

    /// Test-only: simulate `on_confirmation_required`.
    fn apply_device_link_request_received_for_test(
        &self,
        device_name: String,
        confirmation_code: String,
        challenge_hex: String,
    ) -> Result<(), MobileError>;

    /// Test-only: simulate `on_failed("qr_expired")`.
    fn apply_device_link_qr_expired_for_test(&self) -> Result<(), MobileError>;

    /// Test-only: returns `true` when a device-link initiator session
    /// is currently held. Used to assert lifecycle correctness around
    /// navigation in/out of the device-linking screen.
    fn device_link_session_is_active_for_test(&self) -> bool;

    /// Test-only: cancel the active session without leaving the
    /// device-linking screen. Bridge-forwarding tests use this to
    /// stop the live cycle thread from racing the test driver
    /// before asserting state pushed via the
    /// `apply_device_link_*_for_test` helpers.
    fn cancel_device_link_session_for_test(&self);

    /// Test-only: navigate to `screen_json` (an `AppScreen` JSON) and
    /// return the screen envelope, driving `after_screen_transition` so
    /// session-lifecycle side effects fire. Replaces the retired public
    /// `navigate_to_json` binding (ADR-043 Am4): the production surface no
    /// longer lets frontends construct domain targets, but device-link
    /// listener tests still need a forward-nav seam to land on
    /// `DeviceLinking` and assert the session spawn/cancel lifecycle.
    fn navigate_to_json_for_test(&self, screen_json: String) -> Result<String, MobileError>;
}

impl PlatformAppEngineTestHelpers for PlatformAppEngine {
    fn save_test_contact(&self, contact: &vauchi_core::Contact) -> Result<(), MobileError> {
        let engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("engine lock poisoned: {e}"),
        })?;
        engine
            .vauchi()
            .storage()
            .save_contact(contact)
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })
    }

    fn save_test_delivery_record(
        &self,
        record: &vauchi_core::storage::DeliveryRecord,
    ) -> Result<(), MobileError> {
        let engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("engine lock poisoned: {e}"),
        })?;
        engine
            .vauchi()
            .storage()
            .create_delivery_record(record)
            .map_err(|e| MobileError::StorageError {
                detail: e.to_string(),
            })
    }

    fn apply_device_link_request_received_for_test(
        &self,
        device_name: String,
        confirmation_code: String,
        challenge_hex: String,
    ) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_request_received(device_name, confirmation_code, challenge_hex);
        Ok(())
    }

    fn apply_device_link_qr_expired_for_test(&self) -> Result<(), MobileError> {
        let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
            detail: format!("Lock failed: {e}"),
        })?;
        let _ = engine.device_link_qr_expired();
        Ok(())
    }

    fn device_link_session_is_active_for_test(&self) -> bool {
        self.engine()
            .lock()
            .map(|e| e.device_link_initiator_active())
            .unwrap_or(false)
    }

    fn cancel_device_link_session_for_test(&self) {
        if let Ok(mut e) = self.engine().lock() {
            e.cancel_device_link_session();
        }
    }

    fn navigate_to_json_for_test(&self, screen_json: String) -> Result<String, MobileError> {
        use crate::json_helpers::{app_screen_from_json, screen_envelope_to_json};
        let screen = app_screen_from_json(&screen_json)?;
        let pre_screen = self
            .engine()
            .lock()
            .map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?
            .current_app_screen()
            .clone();
        let (model, pending_commands) = {
            let mut engine = self.engine().lock().map_err(|e| MobileError::Other {
                detail: format!("Lock failed: {e}"),
            })?;
            let model = engine.navigate_to(screen);
            let cmds = engine.drain_pending_commands();
            (model, cmds)
        };
        self.after_screen_transition(pre_screen)?;
        screen_envelope_to_json(&model, &pending_commands)
    }
}
