// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Host-side socket for local device linking (ADR-070 Phase 1).
//!
//! Drives the real loopback socket rather than a fake: the socket *is* the
//! boundary under test, and the properties that matter — a silent peer
//! cannot wedge the loop, the listener stops on expiry — only exist at that
//! layer.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use vauchi_app::orchestrator::local_listener::{
    ListenerRuntime, LocalRendezvousListener, mint_code,
};
use vauchi_app::orchestrator::local_rendezvous::SingleCeremonyRendezvous;
use vauchi_app::orchestrator::local_wire::LocalResponse;
use vauchi_core::monotonic::SystemMonotonicClock;
use vauchi_core::rng::{OsSecureRng, SecureRng};
use vauchi_core::sleeper::SystemSleeper;

const OFFERED: &str = "b64-initiator-payload";
const RESPONSE: &str = "b64-joiner-payload";
const READ_TIMEOUT: Duration = Duration::from_millis(150);
const WINDOW: Duration = Duration::from_secs(30);

fn listener(window: Duration) -> LocalRendezvousListener {
    LocalRendezvousListener::bind(
        Arc::new(SingleCeremonyRendezvous::new()),
        ListenerRuntime {
            rng: OsSecureRng::shared(),
            clock: SystemMonotonicClock::shared(),
            sleeper: SystemSleeper::shared(),
        },
        window,
        READ_TIMEOUT,
    )
    .expect("binds on loopback")
}

/// Connect, write one frame, half-close, read the reply — the protocol the
/// module documents.
fn request(addr: SocketAddr, frame: &serde_json::Value) -> LocalResponse {
    let mut stream = TcpStream::connect(addr).expect("connects");
    stream
        .write_all(&serde_json::to_vec(frame).expect("frame encodes"))
        .expect("writes");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("half-close");
    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).expect("reads reply");
    serde_json::from_slice(&reply).expect("reply is a LocalResponse")
}

fn offer(payload: &str) -> serde_json::Value {
    serde_json::json!({ "action": "exchange_offer", "payload": payload })
}

// @scenario: local_device_link :: a ceremony completes over a real socket
#[test]
fn a_ceremony_runs_over_the_socket() {
    let host = listener(WINDOW);
    let addr = host.addr();

    let code = match request(addr, &offer(OFFERED)) {
        LocalResponse::Offered { code } => code,
        other => panic!("expected Offered, got {other:?}"),
    };

    assert_eq!(
        request(
            addr,
            &serde_json::json!({ "action": "exchange_complete", "code": code })
        ),
        LocalResponse::Polled { response: None },
        "an unclaimed ceremony polls as pending"
    );

    assert_eq!(
        request(
            addr,
            &serde_json::json!({ "action": "exchange_claim", "code": code, "response": RESPONSE })
        ),
        LocalResponse::Claimed {
            payload: OFFERED.to_string()
        }
    );

    assert_eq!(
        request(
            addr,
            &serde_json::json!({ "action": "exchange_complete", "code": code })
        ),
        LocalResponse::Polled {
            response: Some(RESPONSE.to_string())
        }
    );
}

// @scenario: local_device_link :: a silent peer does not wedge the listener
#[test]
fn a_peer_that_connects_and_says_nothing_does_not_wedge_the_listener() {
    let host = listener(WINDOW);
    let addr = host.addr();

    // Held open deliberately, never written to, never closed.
    let _silent = TcpStream::connect(addr).expect("silent peer connects");

    // If the read timeout did not bound the silent connection, this would
    // never be answered — the accept loop serves one peer at a time.
    assert!(
        matches!(
            request(addr, &offer(OFFERED)),
            LocalResponse::Offered { .. }
        ),
        "the next peer must still be served"
    );
}

// @scenario: local_device_link :: the socket closes when the window expires
#[test]
fn the_listener_stops_when_the_qr_window_expires() {
    let host = listener(Duration::from_millis(100));
    let addr = host.addr();

    // Poll for the observable outcome rather than sleeping a fixed span
    // (CC-06): the window is short, so this settles in a few attempts.
    let mut refused_after_expiry = false;
    for _ in 0..100 {
        match TcpStream::connect(addr) {
            Ok(mut open) => {
                // The port may still accept from the backlog after the loop
                // exits; a served ceremony would answer, an expired one
                // gives nothing back.
                let _ = open.write_all(b"{\"action\":\"exchange_offer\",\"payload\":\"x\"}");
                let _ = open.shutdown(std::net::Shutdown::Write);
                let mut reply = Vec::new();
                let _ = open.read_to_end(&mut reply);
                if reply.is_empty() {
                    refused_after_expiry = true;
                    break;
                }
            }
            Err(_) => {
                refused_after_expiry = true;
                break;
            }
        }
        std::thread::yield_now();
    }

    assert!(
        refused_after_expiry,
        "the listener must stop serving once the QR window has elapsed"
    );
}

// @scenario: local_device_link :: minted codes are unguessable
#[test]
fn minted_codes_carry_full_entropy_and_do_not_repeat() {
    let rng: Arc<dyn SecureRng> = OsSecureRng::shared();

    let codes: std::collections::HashSet<String> =
        (0..64).map(|_| mint_code(rng.as_ref())).collect();

    assert_eq!(codes.len(), 64, "minted codes must not collide");
    for code in &codes {
        // 16 bytes hex-encoded. The relay's six digits are only safe
        // because it rate-limits claims; a local host has no limiter.
        assert_eq!(code.len(), 32, "expected 128 bits of entropy, got {code}");
        assert!(code.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

// @scenario: local_device_link :: a frame split across writes is still served
// Regression: accepted sockets inherit the listener's non-blocking flag on
// macOS/BSD but not on Linux, so a read returned `WouldBlock` the instant no
// byte was buffered and the peer was dropped without a reply — intermittently,
// and only on some platforms. A real peer's frame can arrive in pieces.
#[test]
fn a_frame_that_arrives_in_two_writes_is_still_served() {
    let host = listener(WINDOW);
    let mut stream = TcpStream::connect(host.addr()).expect("connects");

    let frame = serde_json::to_vec(&offer(OFFERED)).expect("frame encodes");
    let (head, tail) = frame.split_at(frame.len() / 2);
    stream.write_all(head).expect("writes head");
    stream.flush().expect("flushes head");
    stream.write_all(tail).expect("writes tail");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("half-close");

    let mut reply = Vec::new();
    stream.read_to_end(&mut reply).expect("reads reply");
    let response: LocalResponse = serde_json::from_slice(&reply).expect("reply decodes");

    assert!(
        matches!(response, LocalResponse::Offered { .. }),
        "a split frame must still be answered, got {response:?}"
    );
}
