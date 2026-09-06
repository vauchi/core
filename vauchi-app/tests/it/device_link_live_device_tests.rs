// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Joining a ceremony hosted by a *real* device over a real network
//! (ADR-070 Phase 1).
//!
//! Ignored by default and inert without `VAUCHI_DEVICE_INVITATION`: it needs
//! a second device on the same segment showing a device-link QR. Everything
//! else covering this path either shares a rendezvous in-process or drives
//! the wire protocol directly, so this is the only check that puts the real
//! responder machine, the real client broker, and a real peer together.
//!
//! Run it with the invitation decoded from the host's QR:
//!
//! ```sh
//! VAUCHI_DEVICE_INVITATION='vauchi://device-link?qr=…&code=…&local=…' \
//!   cargo nextest run -p vauchi-app --features test-kdf,network-http \
//!   -E 'test(a_real_joiner)' --run-ignored all
//! ```

#![cfg(all(feature = "network-http", feature = "storage"))]

use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use vauchi_app::orchestrator::device_link_responder_machine::{
    DeviceLinkResponderMachine, ResponderEvent,
};
use vauchi_app::orchestrator::local_client::RemoteRendezvousBroker;
use vauchi_core::exchange::DeviceLinkJoinInvitation;

const TIMEOUT: Duration = Duration::from_secs(5);

// @scenario: device_management :: a joiner links against a real hosting device
#[test]
#[ignore = "needs a live host device; set VAUCHI_DEVICE_INVITATION"]
fn a_real_joiner_posts_its_request_to_a_hosting_device() {
    let url = std::env::var("VAUCHI_DEVICE_INVITATION")
        .expect("set VAUCHI_DEVICE_INVITATION to the host's decoded QR");

    let invitation = DeviceLinkJoinInvitation::parse_url(&url).expect("the invitation parses");
    let advertised = invitation
        .local_rendezvous
        .clone()
        .expect("the host must be advertising a local rendezvous");
    let addr: SocketAddr = advertised.parse().expect("advertised as host:port");

    let broker = RemoteRendezvousBroker::new(addr, TIMEOUT, TIMEOUT);
    let mut joiner = DeviceLinkResponderMachine::new(invitation, "Mac Joiner".to_string(), 300)
        .expect("the responder machine builds from the invitation");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock is after the epoch")
        .as_secs();

    match joiner.advance(&broker, now) {
        ResponderEvent::RequestPosted { confirmation_code } => {
            // The host shows the same code; a human compares them. Printed so
            // the operator can check it against the screen.
            println!("CONFIRMATION CODE FROM JOINER: {confirmation_code}");
            assert!(
                !confirmation_code.is_empty(),
                "a posted request must carry a confirmation code to compare"
            );
        }
        other => panic!("expected RequestPosted against {addr}, got {other:?}"),
    }
}
