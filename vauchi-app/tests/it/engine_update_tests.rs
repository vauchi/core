// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! `WorkflowEngine::apply_update` — typed hub→engine channel
//! (`2026-06-10-appengine-typed-engine-channel` Phase 2).
//!
//! Per cluster: the matching engine consumes the update (`true`) and
//! the state effect is observable; a mismatched engine reports `false`
//! and stays untouched (CC-03 positive + negative).

use vauchi_app::ui::{
    DeviceLinkUpdate, DeviceLinkingEngine, EngineOutput, EngineUpdate, FingerprintVerifyEngine,
    LinkExchangeEngine, LinkExchangeUpdate, MultiStageExchangeEngine, MultiStageUpdate,
    VerifyAction, WorkflowEngine,
};

// @scenario: exchange.feature - Multi-stage exchange completes
#[test]
fn multi_stage_consumes_finalized_update() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    assert!(
        engine.apply_update(EngineUpdate::MultiStage(MultiStageUpdate::Finalized(
            "Ada".into()
        )))
    );
    assert!(engine.apply_update(EngineUpdate::MultiStage(MultiStageUpdate::SessionEnded)));
    // Terminal-screen rendering of the peer name is covered by the
    // multi_stage behavior suites; this test owns the channel contract.
}

// @scenario: exchange.feature - Multi-stage exchange completes
#[test]
fn multi_stage_rejects_foreign_update() {
    let mut engine = MultiStageExchangeEngine::new_glance();
    assert!(!engine.apply_update(EngineUpdate::BleForceSuccess));
    assert!(!engine.apply_update(EngineUpdate::ConfirmPendingDelete));
}

// @scenario: exchange.feature - Hover mode exchange
#[test]
fn multi_stage_hover_mode_exposed_via_output() {
    assert_eq!(
        MultiStageExchangeEngine::new_hover().engine_output(),
        Some(EngineOutput::MultiStageExchange { hover_mode: true })
    );
    assert_eq!(
        MultiStageExchangeEngine::new_glance().engine_output(),
        Some(EngineOutput::MultiStageExchange { hover_mode: false })
    );
}

// @scenario: device_link.feature - Device link QR expires
#[test]
fn device_link_consumes_transitions_and_rejects_foreign() {
    let mut engine = DeviceLinkingEngine::new(String::new());
    assert!(
        engine.apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::QrReady {
            qr_data: "QRDATA".into(),
            expires_at: 1_700_000_000,
        }))
    );
    assert!(engine.apply_update(EngineUpdate::DeviceLink(DeviceLinkUpdate::QrExpired)));
    let json = serde_json::to_string(&engine.current_screen()).expect("screen serializes");
    assert!(
        json.contains("expired") || json.contains("Expired"),
        "QR-expired state must reach the screen: {json}"
    );
    assert!(!engine.apply_update(EngineUpdate::BleForceSuccess));
}

// @scenario: exchange.feature - Link exchange shares a URL
#[test]
fn link_exchange_consumes_share_url_and_failure() {
    let mut engine = LinkExchangeEngine::new();
    assert!(
        engine.apply_update(EngineUpdate::LinkExchange(LinkExchangeUpdate::ShareUrl(
            "https://vauchi.app/x/abc".into()
        )))
    );
    assert!(
        engine.apply_update(EngineUpdate::LinkExchange(LinkExchangeUpdate::Failed(
            "polling_timeout".into()
        )))
    );
    assert!(!engine.apply_update(EngineUpdate::ConfirmPendingDelete));
}

// @scenario: fingerprint.feature - Fingerprint screen shows both fingerprints
#[test]
fn engines_without_apply_update_reject_everything() {
    let mut engine = FingerprintVerifyEngine::new("c1", "AAAA", "BBBB", false);
    assert!(!engine.apply_update(EngineUpdate::BleForceSuccess));
    assert_eq!(
        engine.engine_output(),
        Some(EngineOutput::FingerprintVerify(VerifyAction::None)),
        "rejected update must not disturb engine state"
    );
}
