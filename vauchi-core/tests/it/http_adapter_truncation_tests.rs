// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

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

const ONE_BLOB_PAGE: &str =
    r#"{"status":"ok","blobs":[{"blob_id":"b1","ciphertext":"AQID","created_at":7}]}"#;

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
