// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! A capped fetch page is reported so the receive phase knows to fetch
//! again, and a rate limit part-way through a fetch keeps what already
//! arrived (vauchi/private#522).

use super::common::mock_relay::{CannedResponse, MockRelay};
use vauchi_core::network::{
    HttpTransport, HttpTransportAdapter, HttpTransportConfig, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, ProxyConfig, RegisterMailbox, Transport, TransportConfig,
};

fn adapter_with_a_token(relay: &MockRelay) -> HttpTransportAdapter {
    adapter_with_tokens(relay, 1)
}

fn adapter_with_tokens(relay: &MockRelay, count: usize) -> HttpTransportAdapter {
    let mut adapter = HttpTransportAdapter::new(HttpTransport::new(HttpTransportConfig {
        relay_url: relay.url(),
        timeout_ms: 5_000,
        proxy: ProxyConfig::None,
        allow_direct: true,
        pinned_certs: vec![],
    }));
    adapter.connect(&TransportConfig::default()).unwrap();
    adapter
        .send(&MessageEnvelope {
            version: PROTOCOL_VERSION,
            message_id: "reg".to_string().into(),
            timestamp: 0,
            payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                tokens: (0..count).map(|i| format!("{i:064x}")).collect(),
            }),
        })
        .unwrap();
    adapter
}

// @internal
#[test]
fn a_capped_fetch_page_is_reported_as_truncated() {
    for truncated in [true, false] {
        let relay = MockRelay::start();
        relay.queue(
            "fetch",
            CannedResponse::ok_json(format!(
                r#"{{"status":"ok","blobs":[],"truncated":{truncated}}}"#
            )),
        );
        let mut adapter = adapter_with_a_token(&relay);

        adapter.receive().unwrap();

        assert_eq!(adapter.last_fetch_truncated(), truncated);
    }
}

// @claim: S4
/// Each registration batch holds one day's tokens; a fetch request that
/// mixed two batches would hand the relay both days' tokens in one set and
/// let it chain the fetcher across days (private#890).
// @scenario: sealed_mailbox :: Fetch requests never span two day epochs
#[test]
fn a_fetch_request_never_spans_two_registration_batches() {
    let relay = MockRelay::start();
    relay.set_default(CannedResponse::ok_json(
        r#"{"status":"ok","blobs":[],"truncated":false}"#,
    ));
    let mut adapter = adapter_with_tokens(&relay, 0);
    let yesterday: Vec<String> = (0..3).map(|i| format!("{i:064x}")).collect();
    let today: Vec<String> = (100..103).map(|i| format!("{i:064x}")).collect();
    for batch in [&yesterday, &today] {
        adapter
            .send(&MessageEnvelope {
                version: PROTOCOL_VERSION,
                message_id: "reg".to_string().into(),
                timestamp: 0,
                payload: MessagePayload::RegisterMailbox(RegisterMailbox {
                    tokens: batch.clone(),
                }),
            })
            .unwrap();
    }

    assert!(adapter.receive().unwrap().is_none());

    let requested: Vec<Vec<String>> = relay
        .received()
        .into_iter()
        .filter(|request| request.path == "/v2/fetch")
        .map(|request| {
            let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
            body["mailbox_tokens"]
                .as_array()
                .unwrap()
                .iter()
                .map(|token| token.as_str().unwrap().to_string())
                .collect()
        })
        .collect();
    assert_eq!(requested.len(), 2, "one request per batch: {requested:?}");
    for tokens in &requested {
        let within_one_batch = tokens.iter().all(|t| yesterday.contains(t))
            || tokens.iter().all(|t| today.contains(t));
        assert!(within_one_batch, "a request spans two batches: {tokens:?}");
    }
    let all: Vec<&String> = requested.iter().flatten().collect();
    assert!(yesterday.iter().chain(&today).all(|t| all.contains(&t)));
}

const ONE_BLOB_PAGE: &str = r#"{"status":"ok","blobs":[{"blob_id":"b1","ciphertext":"AQID","created_at":7,"mailbox_token":"0000000000000000000000000000000000000000000000000000000000000000"}]}"#;

// @internal
#[test]
fn a_rate_limit_after_a_delivered_chunk_keeps_what_arrived() {
    let relay = MockRelay::start();
    relay.queue("fetch", CannedResponse::ok_json(ONE_BLOB_PAGE));
    relay.queue("fetch", CannedResponse::rate_limited(Some(30)));
    let mut adapter = adapter_with_tokens(&relay, 101);

    let envelope = adapter.receive().unwrap().expect("the first chunk's blob");

    assert_eq!(envelope.message_id.to_string(), "b1");
    assert_eq!(envelope.timestamp, 7);
    let MessagePayload::EncryptedUpdate(update) = envelope.payload else {
        panic!("expected the fetched update");
    };
    assert_eq!(update.recipient_id.as_str(), "0".repeat(64));
    assert!(adapter.receive().unwrap().is_none());
}

// @internal
#[test]
fn a_rate_limit_before_anything_arrived_is_an_error() {
    let relay = MockRelay::start();
    relay.queue("fetch", CannedResponse::rate_limited(Some(30)));
    let mut adapter = adapter_with_tokens(&relay, 101);

    let err = adapter.receive().unwrap_err();

    assert!(
        matches!(err, vauchi_core::network::NetworkError::RateLimited { .. }),
        "{err:?}"
    );
}
