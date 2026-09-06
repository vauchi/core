// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Joiner-side broker for local device linking (ADR-070 Phase 1).
//!
//! Driven against the real host listener rather than a fake: the socket is
//! the thing under test, and the properties that matter — bounded reads, a
//! peer that never answers, an address that is not there — only exist at
//! that layer.

#![cfg(feature = "network-http")]

use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::sync::Arc;
use std::time::Duration;

use vauchi_app::orchestrator::device_link_relay::DeviceLinkBroker;
use vauchi_app::orchestrator::local_client::RemoteRendezvousBroker;
use vauchi_app::orchestrator::local_listener::{ListenerRuntime, LocalRendezvousListener};
use vauchi_app::orchestrator::local_rendezvous::SingleCeremonyRendezvous;
use vauchi_core::monotonic::SystemMonotonicClock;
use vauchi_core::network::NetworkError;
use vauchi_core::rng::OsSecureRng;
use vauchi_core::sleeper::SystemSleeper;

const OFFERED: &str = "b64-initiator-payload";
const RESPONSE: &str = "b64-joiner-payload";
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const READ_TIMEOUT: Duration = Duration::from_millis(500);

fn host() -> LocalRendezvousListener {
    LocalRendezvousListener::bind(
        Arc::new(SingleCeremonyRendezvous::new()),
        ListenerRuntime {
            rng: OsSecureRng::shared(),
            clock: SystemMonotonicClock::shared(),
            sleeper: SystemSleeper::shared(),
        },
        Duration::from_secs(30),
        Duration::from_millis(150),
    )
    .expect("host binds")
}

fn broker_for(addr: SocketAddr) -> RemoteRendezvousBroker {
    RemoteRendezvousBroker::new(addr, CONNECT_TIMEOUT, READ_TIMEOUT)
}

// @scenario: local_device_link :: the joiner drives a ceremony over the socket
#[test]
fn the_joiner_broker_runs_offer_claim_and_complete_over_the_socket() {
    let listener = host();
    let broker = broker_for(listener.addr());

    let code = broker
        .exchange_offer(OFFERED, Some(300))
        .expect("offer accepted");

    assert_eq!(
        broker.exchange_complete(&code).expect("poll before claim"),
        None,
        "an unclaimed ceremony polls as pending"
    );
    assert_eq!(
        broker.exchange_claim(&code, RESPONSE).expect("claim"),
        OFFERED
    );
    assert_eq!(
        broker.exchange_complete(&code).expect("poll after claim"),
        Some(RESPONSE.to_string())
    );
}

// @scenario: local_device_link :: a refusal reaches the joiner as an error
#[test]
fn a_refused_request_surfaces_as_an_error_not_a_success() {
    let listener = host();
    let broker = broker_for(listener.addr());

    let err = broker
        .exchange_claim("no-such-ceremony", RESPONSE)
        .expect_err("an unknown code must be refused");

    assert!(
        matches!(err, NetworkError::InvalidMessage(_)),
        "expected a protocol-level error, got {err:?}"
    );
}

// @scenario: local_device_link :: an unreachable rendezvous fails rather than hangs
#[test]
fn an_address_with_nothing_listening_fails_instead_of_hanging() {
    // Bind then drop, so the port is real but certainly unserved.
    let dead = {
        let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
        probe.local_addr().expect("has an address")
    };

    let err = broker_for(dead)
        .exchange_offer(OFFERED, None)
        .expect_err("a dead address must not appear to succeed");

    assert!(
        matches!(
            err,
            NetworkError::ConnectionFailed(_) | NetworkError::SendFailed(_)
        ),
        "expected a connection failure, got {err:?}"
    );
}

// @scenario: local_device_link :: a peer that accepts and never answers times out
#[test]
fn a_peer_that_accepts_but_never_answers_times_out() {
    // Accepts connections and does nothing else — the shape a hostile or
    // wedged host presents. Left bound for the test's lifetime.
    let silent = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("binds");
    let addr = silent.local_addr().expect("has an address");

    let err = broker_for(addr)
        .exchange_offer(OFFERED, None)
        .expect_err("a silent peer must not stall the ceremony forever");

    assert!(
        matches!(
            err,
            NetworkError::ReceiveFailed(_) | NetworkError::InvalidMessage(_)
        ),
        "expected a receive failure, got {err:?}"
    );
}
