// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Response-handling tests for `HttpTransport` using `MockRelay`.
//!
//! Drives the in-process mock relay (see `common/mock_relay.rs`) to
//! cover the post-network branches of `http_transport.rs` that
//! `http_transport_endpoint_tests.rs` couldn't reach: status code
//! mapping (200/426/429/other), version-policy header handling,
//! rate-limit `Retry-After`, and JSON response field round-trips.

#![cfg(feature = "network-http")]

use vauchi_core::network::NetworkError;
use vauchi_core::network::http_transport::{HttpTransport, HttpTransportConfig};
use vauchi_protocol::v2::{V2GuardianEntry, V2Response};

use crate::common::mock_relay::{CannedResponse, MockRelay};

fn transport_pointing_at(mock: &MockRelay) -> HttpTransport {
    HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), 2_000))
}

fn ok_response_with(f: impl FnOnce(&mut V2Response)) -> CannedResponse {
    let mut r = V2Response::new("ok");
    f(&mut r);
    CannedResponse::ok_v2_response(&r)
}

fn err_response_with(message: &str) -> CannedResponse {
    let mut r = V2Response::new("error");
    r.error = Some(message.to_string());
    CannedResponse::ok_v2_response(&r)
}

// ============================================================
// acknowledge — success returns inner `acknowledged` flag
// ============================================================

// @internal
#[test]
fn acknowledge_returns_true_when_relay_acknowledges() {
    let mock = MockRelay::start();
    mock.queue("ack", ok_response_with(|r| r.acknowledged = Some(true)));

    let transport = transport_pointing_at(&mock);
    let result = transport.acknowledge("recipient-1", "blob-abc").unwrap();

    assert!(result, "relay returned acknowledged=true");
    let req = mock.last_received();
    assert_eq!(req.method, "POST");
    assert_eq!(req.path, "/v2/ack");
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["recipient_id"], "recipient-1");
    assert_eq!(body["blob_id"], "blob-abc");
}

// @internal
#[test]
fn acknowledge_returns_false_when_acknowledged_field_missing() {
    let mock = MockRelay::start();
    mock.queue("ack", ok_response_with(|_| {})); // status=ok, no `acknowledged`

    let transport = transport_pointing_at(&mock);
    let result = transport.acknowledge("r", "b").unwrap();

    assert!(
        !result,
        "missing `acknowledged` must default to false (unwrap_or)"
    );
}

// @internal
#[test]
fn acknowledge_maps_error_status_to_invalid_message() {
    let mock = MockRelay::start();
    mock.queue("ack", err_response_with("blob not found"));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();

    match err {
        NetworkError::InvalidMessage(msg) => assert!(
            msg.contains("ack failed") && msg.contains("blob not found"),
            "expected wrapped action+message, got: {msg}"
        ),
        other => panic!("expected InvalidMessage, got {other:?}"),
    }
}

// @internal
#[test]
fn acknowledge_detects_rate_limit_in_error_message() {
    let mock = MockRelay::start();
    // 200 OK body, but "rate limit" in the error string.
    mock.queue("ack", err_response_with("rate limit exceeded"));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();

    assert!(
        matches!(err, NetworkError::RateLimited { .. }),
        "rate-limit string in error must map to RateLimited variant; got {err:?}"
    );
}

// @internal
#[test]
fn acknowledge_detects_quota_exceeded_in_error_message() {
    let mock = MockRelay::start();
    mock.queue("ack", err_response_with("quota exceeded for today"));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    assert!(matches!(err, NetworkError::RateLimited { .. }));
}

// ============================================================
// purge — returns blobs_deleted count
// ============================================================

// @internal
#[test]
fn purge_returns_blobs_deleted_count() {
    let mock = MockRelay::start();
    mock.queue("purge", ok_response_with(|r| r.blobs_deleted = Some(7)));

    let transport = transport_pointing_at(&mock);
    let count = transport
        .purge("recipient-x", "pubkey", "token", "sig", 1_700_000_000)
        .unwrap();

    assert_eq!(count, Some(7));
    let req = mock.last_received();
    let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["recipient_id"], "recipient-x");
    assert_eq!(body["public_key"], "pubkey");
    assert_eq!(body["purge_token"], "token");
    assert_eq!(body["signature"], "sig");
    assert_eq!(body["timestamp"], 1_700_000_000u64);
}

// @internal
#[test]
fn purge_with_missing_count_returns_none() {
    let mock = MockRelay::start();
    mock.queue("purge", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    let count = transport.purge("r", "p", "t", "s", 1).unwrap();
    assert_eq!(count, None);
}

// ============================================================
// recovery_store — Ok(()) on success, error mapping on failure
// ============================================================

// @internal
#[test]
fn recovery_store_returns_unit_on_success() {
    let mock = MockRelay::start();
    mock.queue("recovery_store", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    let result = transport.recovery_store("deadbeef".repeat(8).as_str(), "cHJvb2Y=");
    assert!(result.is_ok());
}

// @internal
#[test]
fn recovery_store_propagates_relay_error_message() {
    let mock = MockRelay::start();
    mock.queue("recovery_store", err_response_with("invalid hash"));

    let transport = transport_pointing_at(&mock);
    let err = transport.recovery_store("abc", "xyz").unwrap_err();
    match err {
        NetworkError::InvalidMessage(msg) => {
            assert!(msg.contains("recovery_store failed") && msg.contains("invalid hash"))
        }
        other => panic!("expected InvalidMessage, got {other:?}"),
    }
}

// ============================================================
// guardian_store / guardian_query / guardian_delete
// ============================================================

// @internal
#[test]
fn guardian_store_succeeds_when_relay_returns_ok() {
    let mock = MockRelay::start();
    mock.queue("guardian_store", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    let entries = vec![V2GuardianEntry {
        data: "ZW50cnk=".to_string(),
    }];
    transport.guardian_store("guardian-hash", entries).unwrap();
    assert_eq!(mock.last_received().path, "/v2/guardian_store");
}

// @internal
#[test]
fn guardian_query_returns_entries_from_response() {
    let mock = MockRelay::start();
    mock.queue(
        "guardian_query",
        ok_response_with(|r| {
            r.guardians = Some(vec![
                V2GuardianEntry {
                    data: "ZW50cnktYQ==".to_string(),
                },
                V2GuardianEntry {
                    data: "ZW50cnktYg==".to_string(),
                },
            ])
        }),
    );

    let transport = transport_pointing_at(&mock);
    let entries = transport.guardian_query("guardian-hash").unwrap();

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].data, "ZW50cnktYQ==");
    assert_eq!(entries[1].data, "ZW50cnktYg==");
}

// @internal
#[test]
fn guardian_query_returns_empty_when_relay_omits_guardians_field() {
    let mock = MockRelay::start();
    mock.queue("guardian_query", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    let entries = transport.guardian_query("guardian-hash").unwrap();
    assert!(
        entries.is_empty(),
        "missing guardians field must surface as empty vec"
    );
}

// @internal
#[test]
fn guardian_delete_succeeds_when_relay_returns_ok() {
    let mock = MockRelay::start();
    mock.queue("guardian_delete", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    transport.guardian_delete("guardian-hash").unwrap();
    assert_eq!(mock.last_received().path, "/v2/guardian_delete");
}

// ============================================================
// exchange_offer / exchange_claim / exchange_complete
// ============================================================

// @internal
#[test]
fn exchange_offer_returns_code_from_response() {
    let mock = MockRelay::start();
    mock.queue(
        "exchange_offer",
        ok_response_with(|r| r.code = Some("123456".into())),
    );

    let transport = transport_pointing_at(&mock);
    let code = transport.exchange_offer("cGF5bG9hZA==", Some(300)).unwrap();
    assert_eq!(code, "123456");
}

// @internal
#[test]
fn exchange_claim_returns_payload_from_response() {
    let mock = MockRelay::start();
    mock.queue(
        "exchange_claim",
        ok_response_with(|r| r.payload = Some("b2ZmZXI=".into())),
    );

    let transport = transport_pointing_at(&mock);
    let payload = transport.exchange_claim("123456", "cmVzcA==").unwrap();
    assert_eq!(payload, "b2ZmZXI=");
}

// @internal
#[test]
fn exchange_complete_returns_some_response_when_set() {
    let mock = MockRelay::start();
    mock.queue(
        "exchange_complete",
        ok_response_with(|r| r.response = Some("cmVzcG9uc2U=".into())),
    );

    let transport = transport_pointing_at(&mock);
    let response = transport.exchange_complete("123456").unwrap();
    assert_eq!(response, Some("cmVzcG9uc2U=".into()));
}

// @internal
#[test]
fn exchange_complete_returns_none_when_response_field_missing() {
    let mock = MockRelay::start();
    mock.queue("exchange_complete", ok_response_with(|_| {}));

    let transport = transport_pointing_at(&mock);
    let response = transport.exchange_complete("123456").unwrap();
    assert_eq!(
        response, None,
        "missing response field → None (still polling)"
    );
}

// ============================================================
// Cross-cutting: HTTP status code → NetworkError mapping
// ============================================================

// @internal
#[test]
fn http_429_maps_to_rate_limited_with_retry_after() {
    let mock = MockRelay::start();
    mock.queue("ack", CannedResponse::rate_limited(Some(45)));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    match err {
        NetworkError::RateLimited { retry_after_secs } => {
            assert_eq!(retry_after_secs, 45, "Retry-After header must be parsed")
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

// @internal
#[test]
fn http_429_without_retry_after_falls_back_to_default() {
    let mock = MockRelay::start();
    mock.queue("ack", CannedResponse::rate_limited(None));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    match err {
        NetworkError::RateLimited { retry_after_secs } => {
            // Default rate-limit retry is non-zero — exact value lives in the
            // implementation; assert it's a sensible positive number.
            assert!(retry_after_secs > 0);
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

// @internal
#[test]
fn http_426_maps_to_upgrade_required_with_min_version() {
    let mock = MockRelay::start();
    mock.queue("ack", CannedResponse::upgrade_required(7));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    match err {
        NetworkError::UpgradeRequired { min_version } => {
            assert_eq!(min_version, 7, "X-Min-Version header must be parsed")
        }
        other => panic!("expected UpgradeRequired, got {other:?}"),
    }
}

// @internal
#[test]
fn http_500_maps_to_connection_failed_carrying_status() {
    let mock = MockRelay::start();
    mock.queue("ack", CannedResponse::status(500));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    match err {
        NetworkError::ConnectionFailed(msg) => {
            assert!(msg.contains("500"), "error must mention HTTP 500: {msg}")
        }
        other => panic!("expected ConnectionFailed, got {other:?}"),
    }
}

// @internal
#[test]
fn http_413_payload_too_large_maps_to_connection_failed() {
    let mock = MockRelay::start();
    mock.queue("ack", CannedResponse::status(413));

    let transport = transport_pointing_at(&mock);
    let err = transport.acknowledge("r", "b").unwrap_err();
    assert!(matches!(err, NetworkError::ConnectionFailed(_)));
}

// ============================================================
// ============================================================

// @internal
#[test]
fn client_sends_app_compat_version_header() {
    let mock = MockRelay::start();
    mock.queue("ack", ok_response_with(|r| r.acknowledged = Some(true)));

    let transport = transport_pointing_at(&mock);
    let _ = transport.acknowledge("r", "b").unwrap();

    let req = mock.last_received();
    let app_compat = req
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("X-App-Compat-Version"));
    assert!(
        app_compat.is_some(),
        "X-App-Compat-Version header must be sent on every request; headers: {:?}",
        req.headers
    );
}
