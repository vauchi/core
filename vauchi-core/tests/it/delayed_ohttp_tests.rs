// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asynchronous OHTTP (vauchi/private#628, design
//! `2026-10-11-outer-relay-delayed-forwarding-design`): fetches and
//! acknowledgements are posted under a ticket and their answers collected on
//! a later poll, so the outer relay can forward them after a random delay.
//! Sends stay synchronous. The mock relay forwards at once and can hold an
//! answer for a number of polls or forget every answer, as a restart does.

#![cfg(feature = "network-http")]

use base64::Engine;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(feature = "testing")]
use vauchi_core::sleeper::FakeSleeper;

use crate::common::mock_relay::{CannedResponse, MockRelay};
use crate::common::signed_gateway::{NOW, SignedGateway};
use vauchi_core::network::http_transport::MAX_PENDING_POLLS;
use vauchi_core::network::{
    AckStatus, Acknowledgment, Collected, DelayedRequests, HttpTransport, HttpTransportAdapter,
    HttpTransportConfig, MessageEnvelope, MessagePayload, OhttpClient, PROTOCOL_VERSION,
    RegisterMailbox, Transport,
};
use vauchi_protocol::ohttp_key::window_of;
use vauchi_protocol::v2::FetchedBlob;

const TIMEOUT_MS: u64 = 2_000;

fn token() -> String {
    "ab".repeat(32)
}

fn blob(id: &str) -> FetchedBlob {
    FetchedBlob {
        blob_id: id.into(),
        ciphertext: base64::engine::general_purpose::STANDARD.encode(b"sealed"),
        created_at: 1,
        mailbox_token: Some(token()),
        origin_hint: None,
    }
}

fn gateway_relay() -> MockRelay {
    let mock = MockRelay::start();
    mock.enable_ohttp_gateway(&SignedGateway::new(0x41), window_of(NOW));
    mock
}

fn transport(mock: &MockRelay) -> HttpTransport {
    let mut http = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), TIMEOUT_MS));
    http.set_ohttp(OhttpClient::new(mock.ohttp_key_config()).expect("key config"));
    http
}

fn adapter(mock: &MockRelay, delayed: &Arc<Mutex<DelayedRequests>>) -> HttpTransportAdapter {
    let mut adapter = HttpTransportAdapter::new(transport(mock)).with_delayed(delayed.clone());
    adapter
        .send(&envelope(MessagePayload::RegisterMailbox(
            RegisterMailbox {
                tokens: vec![token()],
            },
        )))
        .expect("register");
    adapter
}

fn envelope(payload: MessagePayload) -> MessageEnvelope {
    MessageEnvelope {
        version: PROTOCOL_VERSION,
        message_id: "m".to_string().into(),
        timestamp: 0,
        payload,
    }
}

fn requests(mock: &MockRelay) -> Vec<String> {
    mock.received()
        .iter()
        .map(|r| format!("{} {}", r.method, r.path))
        .collect()
}

fn posts(mock: &MockRelay) -> usize {
    requests(mock)
        .iter()
        .filter(|r| r.as_str() == "POST /v2/ohttp-async")
        .count()
}

fn ticket_of(request: &crate::common::mock_relay::ReceivedRequest) -> String {
    request
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("vauchi-ticket"))
        .map(|(_, value)| value.clone())
        .expect("ticket header")
}

// @internal
#[test]
fn a_delayed_fetch_is_answered_through_its_ticket() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    let http = transport(&mock);

    let pending = http.post_delayed_fetch(&[token()]).expect("posted");
    let ticket = pending.ticket().to_string();
    let collected = http.collect_delayed_fetch(pending).expect("collected");

    let Collected::Ready((blobs, truncated)) = collected else {
        panic!("expected the held answer, got {collected:?}");
    };
    assert_eq!(
        blobs.iter().map(|b| b.blob_id.as_str()).collect::<Vec<_>>(),
        vec!["b1"]
    );
    assert!(!truncated);
    assert_eq!(
        requests(&mock),
        vec![
            "POST /v2/ohttp-async".to_string(),
            format!("GET /v2/ohttp-async/{ticket}"),
        ],
        "nothing goes through the synchronous /v2/ohttp"
    );
    assert_eq!(ticket_of(&mock.received()[0]), ticket);
    assert_eq!(mock.ohttp_actions(), vec!["fetch".to_string()]);
}

// @internal
#[test]
fn each_delayed_request_carries_a_fresh_128_bit_ticket() {
    let mock = gateway_relay();
    let http = transport(&mock);

    let first = http.post_delayed_fetch(&[token()]).expect("posted");
    let second = http.post_delayed_fetch(&[token()]).expect("posted");

    for ticket in [first.ticket(), second.ticket()] {
        assert_eq!(ticket.len(), 32, "{ticket}");
        assert!(ticket.bytes().all(|b| b.is_ascii_hexdigit()), "{ticket}");
    }
    assert_ne!(first.ticket(), second.ticket());
}

// @internal
#[test]
fn a_held_answer_stays_pending_and_keeps_its_ticket() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    mock.hold_async_answers(1);
    let http = transport(&mock);
    let pending = http.post_delayed_fetch(&[token()]).expect("posted");

    let Collected::Pending(pending) = http.collect_delayed_fetch(pending).expect("collect") else {
        panic!("a held answer is pending");
    };
    let collected = http.collect_delayed_fetch(pending).expect("collect");

    assert!(
        matches!(&collected, Collected::Ready((blobs, _)) if blobs.len() == 1),
        "{collected:?}"
    );
}

// @internal
#[test]
fn an_answer_the_relay_forgot_is_lost() {
    let mock = gateway_relay();
    let http = transport(&mock);
    let pending = http.post_delayed_fetch(&[token()]).expect("posted");
    mock.forget_async_answers();

    let collected = http.collect_delayed_fetch(pending).expect("collect");

    assert!(matches!(collected, Collected::Lost), "{collected:?}");
}

// @internal
#[test]
fn a_ticket_still_pending_after_the_poll_cap_is_lost() {
    let mock = gateway_relay();
    mock.hold_async_answers(MAX_PENDING_POLLS + 1);
    let http = transport(&mock);
    let mut pending = http.post_delayed_fetch(&[token()]).expect("posted");

    for poll in 0..MAX_PENDING_POLLS {
        pending = match http.collect_delayed_fetch(pending).expect("collect") {
            Collected::Pending(pending) => pending,
            other => panic!("poll {poll}: still pending, got {other:?}"),
        };
    }
    let collected = http.collect_delayed_fetch(pending).expect("collect");

    assert!(matches!(collected, Collected::Lost), "{collected:?}");
}

// @internal
#[test]
fn a_tampered_answer_is_an_error_and_the_fetch_is_posted_again() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    let delayed = DelayedRequests::shared();
    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());
    mock.tamper_async_answers();

    let error = adapter(&mock, &delayed)
        .receive()
        .expect_err("an answer that does not open is reported");

    assert!(
        error
            .to_string()
            .contains("OHTTP response decryption failed"),
        "{error}"
    );
    assert_eq!(posts(&mock), 2, "{:?}", requests(&mock));
}

// @internal
#[test]
fn a_token_registered_while_a_fetch_is_pending_is_fetched_at_once() {
    let mock = gateway_relay();
    mock.hold_async_answers(1);
    let delayed = DelayedRequests::shared();
    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());

    let mut cycle = adapter(&mock, &delayed);
    cycle
        .send(&envelope(MessagePayload::RegisterMailbox(
            RegisterMailbox {
                tokens: vec!["cd".repeat(32)],
            },
        )))
        .expect("register");
    assert!(cycle.receive().expect("r").is_none());

    assert_eq!(posts(&mock), 2, "{:?}", requests(&mock));
    assert_eq!(
        mock.received().last().map(|r| r.method.clone()),
        Some("POST".to_string()),
        "the new token's fetch is posted after the pending one is polled"
    );
}

// @internal
#[test]
fn a_refused_post_is_an_error() {
    let mock = gateway_relay();
    mock.queue("ohttp-async", CannedResponse::status(503));
    let http = transport(&mock);

    let error = http
        .post_delayed_fetch(&[token()])
        .expect_err("a full outer relay refuses the post");

    assert_eq!(error.to_string(), "Connection failed: HTTP 503");
    assert!(mock.ohttp_actions().is_empty());
}

// @internal
#[test]
fn a_delayed_request_needs_ohttp() {
    let mock = gateway_relay();
    let http = HttpTransport::new(HttpTransportConfig::for_testing(mock.url(), TIMEOUT_MS));

    let error = http
        .post_delayed_fetch(&[token()])
        .expect_err("no direct path for delayed requests");

    assert_eq!(error.to_string(), "Transport not connected");
    assert!(mock.received().is_empty());
}

// @internal
#[test]
fn delayed_mail_arrives_on_the_cycle_after_the_fetch_was_posted() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    let delayed = DelayedRequests::shared();

    let mut first_cycle = adapter(&mock, &delayed);
    assert!(first_cycle.receive().expect("receive").is_none());
    drop(first_cycle);

    let mut second_cycle = adapter(&mock, &delayed);
    let mail = second_cycle.receive().expect("receive").expect("mail");

    assert_eq!(mail.message_id.into_string(), "b1");
    assert!(
        !requests(&mock).iter().any(|r| r == "POST /v2/ohttp"),
        "{:?}",
        requests(&mock)
    );
}

// @internal
#[test]
fn no_second_fetch_is_posted_while_one_is_pending() {
    let mock = gateway_relay();
    mock.hold_async_answers(1);
    let delayed = DelayedRequests::shared();

    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());
    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());

    assert_eq!(posts(&mock), 1, "{:?}", requests(&mock));
}

// @internal
#[test]
fn a_lost_fetch_is_posted_again() {
    let mock = gateway_relay();
    let delayed = DelayedRequests::shared();
    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());
    mock.forget_async_answers();

    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());

    assert_eq!(posts(&mock), 2, "{:?}", requests(&mock));
}

// @internal
#[test]
fn acknowledgements_are_posted_through_the_delayed_path() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    let delayed = DelayedRequests::shared();
    assert!(adapter(&mock, &delayed).receive().expect("r").is_none());
    let mut cycle = adapter(&mock, &delayed);
    let mail = cycle.receive().expect("receive").expect("mail");

    cycle
        .send(&envelope(MessagePayload::Acknowledgment(Acknowledgment {
            message_id: mail.message_id,
            status: AckStatus::ReceivedByRecipient,
            error: None,
        })))
        .expect("queue ack");
    assert!(cycle.receive().expect("receive").is_none());

    assert!(
        mock.ohttp_actions().contains(&"ack".to_string()),
        "{:?}",
        mock.ohttp_actions()
    );
    assert!(
        !requests(&mock).iter().any(|r| r == "POST /v2/ohttp"),
        "{:?}",
        requests(&mock)
    );
}

// @internal
#[test]
fn without_delayed_state_the_adapter_fetches_synchronously() {
    let mock = gateway_relay();
    mock.set_gateway_blobs(vec![blob("b1")]);
    let mut adapter = HttpTransportAdapter::new(transport(&mock));
    adapter
        .send(&envelope(MessagePayload::RegisterMailbox(
            RegisterMailbox {
                tokens: vec![token()],
            },
        )))
        .expect("register");

    let mail = adapter.receive().expect("receive").expect("mail");

    assert_eq!(mail.message_id.into_string(), "b1");
    assert_eq!(requests(&mock), vec!["POST /v2/ohttp".to_string()]);
}

#[cfg(feature = "testing")]
fn vauchi_through(
    outer: &MockRelay,
    delayed: bool,
) -> (vauchi_core::api::Vauchi, tempfile::TempDir) {
    vauchi_sleeping_on(outer, delayed, Arc::new(FakeSleeper::new()))
}

#[cfg(feature = "testing")]
fn vauchi_sleeping_on(
    outer: &MockRelay,
    delayed: bool,
    sleeper: Arc<FakeSleeper>,
) -> (vauchi_core::api::Vauchi, tempfile::TempDir) {
    use vauchi_core::api::{Vauchi, VauchiConfig};
    let application = MockRelay::start();
    let dir = tempfile::tempdir().expect("temp dir");
    let mut config = VauchiConfig::with_storage_path(dir.path().join("vauchi.db"))
        .with_storage_key(vauchi_core::crypto::SymmetricKey::generate())
        .with_relay_url(application.url())
        .with_ohttp_relay_url(outer.url());
    config.relay.ohttp_delayed = delayed;
    let mut vauchi = Vauchi::new(config)
        .expect("create Vauchi")
        .with_sleeper(sleeper.clone());
    vauchi.create_identity("Delayed").expect("identity");
    vauchi.set_ohttp_key_for_testing(OhttpClient::new(outer.ohttp_key_config()).expect("key"));
    (vauchi, dir)
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn a_delayed_sync_collects_its_fetch_on_the_next_sync() {
    let outer = gateway_relay();
    let (mut vauchi, _dir) = vauchi_through(&outer, true);

    vauchi.sync().expect("first sync");
    vauchi.set_next_sync_allowed(std::time::Instant::now());
    vauchi.sync().expect("second sync");

    let ticket = ticket_of(
        outer
            .received()
            .iter()
            .find(|r| r.path == "/v2/ohttp-async")
            .expect("a delayed fetch was posted"),
    );
    assert!(
        requests(&outer).contains(&format!("GET /v2/ohttp-async/{ticket}")),
        "the second sync collects the first one's ticket: {:?}",
        requests(&outer)
    );
    let fetches = outer
        .ohttp_actions()
        .iter()
        .filter(|action| action.as_str() == "fetch")
        .count();
    assert_eq!(
        fetches,
        posts(&outer),
        "every fetch went through the delayed path: {:?}",
        requests(&outer)
    );
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn delayed_forwarding_is_off_by_default() {
    assert!(!vauchi_core::api::RelayConfig::default().ohttp_delayed);
    let outer = gateway_relay();
    let (mut vauchi, _dir) = vauchi_through(&outer, false);

    vauchi.sync().expect("sync");

    assert!(
        !requests(&outer)
            .iter()
            .any(|r| r.contains("/v2/ohttp-async")),
        "{:?}",
        requests(&outer)
    );
}

const POLL: Duration = Duration::from_secs(30);
const MAX_WAIT: Duration = Duration::from_secs(960);

#[cfg(feature = "testing")]
fn fetched(outcome: &vauchi_core::api::VauchiSyncOutcome) -> usize {
    match outcome {
        vauchi_core::api::VauchiSyncOutcome::Ok { fetched, .. } => *fetched,
        other => panic!("expected a completed sync, got {other:?}"),
    }
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn a_waiting_sync_returns_the_mail_its_delayed_fetch_brought() {
    let outer = gateway_relay();
    outer.set_gateway_blobs(vec![blob("b1")]);
    outer.hold_async_answers(2);
    let sleeper = Arc::new(FakeSleeper::new());
    let (mut vauchi, _dir) = vauchi_sleeping_on(&outer, true, sleeper.clone());

    let outcome = vauchi
        .sync_collecting_delayed(POLL, MAX_WAIT)
        .expect("sync");

    assert!(
        fetched(&outcome) >= 1,
        "the mail arrived: {outcome:?} sleeps={:?} requests={:?}",
        sleeper.calls(),
        requests(&outer)
    );
    assert!(!sleeper.calls().is_empty());
    assert!(
        sleeper.calls().iter().all(|d| *d == POLL),
        "{:?}",
        sleeper.calls()
    );
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn a_waiting_sync_gives_up_after_its_maximum_wait() {
    let outer = gateway_relay();
    outer.hold_async_answers(u32::MAX);
    let sleeper = Arc::new(FakeSleeper::new());
    let (mut vauchi, _dir) = vauchi_sleeping_on(&outer, true, sleeper.clone());

    let outcome = vauchi
        .sync_collecting_delayed(POLL, 3 * POLL)
        .expect("sync");

    assert_eq!(fetched(&outcome), 0);
    assert_eq!(sleeper.calls(), vec![POLL; 3]);
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn a_lost_answer_is_waited_for_again() {
    let outer = gateway_relay();
    outer.set_gateway_blobs(vec![blob("b1")]);
    outer.hold_async_answers(1);
    let sleeper = Arc::new(FakeSleeper::new());
    let (mut vauchi, _dir) = vauchi_sleeping_on(&outer, true, sleeper.clone());
    vauchi.sync().expect("a sync that posts the first fetch");
    outer.forget_async_answers();
    vauchi.set_next_sync_allowed(std::time::Instant::now());

    let outcome = vauchi
        .sync_collecting_delayed(POLL, MAX_WAIT)
        .expect("sync");

    assert!(fetched(&outcome) >= 1, "the mail arrived: {outcome:?}");
}

// @internal
#[test]
#[cfg(feature = "testing")]
fn without_delayed_mode_a_waiting_sync_never_sleeps() {
    let outer = gateway_relay();
    outer.set_gateway_blobs(vec![blob("b1")]);
    let sleeper = Arc::new(FakeSleeper::new());
    let (mut vauchi, _dir) = vauchi_sleeping_on(&outer, false, sleeper.clone());

    let outcome = vauchi
        .sync_collecting_delayed(POLL, MAX_WAIT)
        .expect("sync");

    assert!(fetched(&outcome) >= 1, "the mail arrived: {outcome:?}");
    assert!(sleeper.calls().is_empty());
}
