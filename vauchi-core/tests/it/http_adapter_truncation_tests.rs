// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A capped fetch page is reported so the receive phase knows to fetch
//! again (vauchi/private#522).

use super::common::mock_relay::{CannedResponse, MockRelay};
use vauchi_core::network::{
    HttpTransport, HttpTransportAdapter, HttpTransportConfig, MessageEnvelope, MessagePayload,
    PROTOCOL_VERSION, ProxyConfig, RegisterMailbox, Transport, TransportConfig,
};

fn adapter_with_a_token(relay: &MockRelay) -> HttpTransportAdapter {
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
                tokens: vec!["t".repeat(64)],
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
