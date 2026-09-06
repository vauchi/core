// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! A join invitation that points at a peer-hosted rendezvous (ADR-070).
//!
//! The address arrives inside a scanned QR, so it is attacker-controlled and
//! is bounded at the parse boundary (DC-01).

use vauchi_core::exchange::DeviceLinkJoinInvitation;

fn invitation_with_local(addr: &str) -> String {
    DeviceLinkJoinInvitation {
        qr_data: "qr-payload".to_string(),
        broker_code: "123456".to_string(),
        relay_url: None,
        local_rendezvous: Some(addr.to_string()),
    }
    .to_url()
}

// @scenario: device_management :: a local rendezvous survives the invitation round trip
#[test]
fn a_local_rendezvous_round_trips_through_the_url() {
    let parsed = DeviceLinkJoinInvitation::parse_url(&invitation_with_local("192.168.1.42:8080"))
        .expect("invitation parses");

    assert_eq!(
        parsed.local_rendezvous.as_deref(),
        Some("192.168.1.42:8080")
    );
    assert_eq!(parsed.relay_url, None, "a local ceremony names no relay");
    assert_eq!(parsed.broker_code, "123456");
}

// @scenario: device_management :: an invitation without a local address still parses
#[test]
fn a_relay_invitation_is_unchanged_by_the_new_field() {
    let url = DeviceLinkJoinInvitation {
        qr_data: "qr-payload".to_string(),
        broker_code: "123456".to_string(),
        relay_url: None,
        local_rendezvous: None,
    }
    .to_url();

    assert!(
        !url.contains("local="),
        "no local param when none is hosted, got {url}"
    );
    let parsed = DeviceLinkJoinInvitation::parse_url(&url).expect("parses");
    assert_eq!(parsed.local_rendezvous, None);
}

// @scenario: device_management :: a globally routable rendezvous is refused
#[test]
fn a_rendezvous_outside_the_local_network_is_refused() {
    // Without this, a crafted QR turns any joiner into a "connect to the
    // host I name" primitive. ADR-070 links in presence, so a routable
    // address is never the right answer.
    for hostile in [
        "203.0.113.5:443",
        "8.8.8.8:80",
        "100.64.0.1:8080",
        "[2001:db8::1]:443",
    ] {
        assert!(
            DeviceLinkJoinInvitation::parse_url(&invitation_with_local(hostile)).is_err(),
            "{hostile} must be refused as a rendezvous"
        );
    }
}

// @scenario: device_management :: link-local rendezvous addresses are accepted
#[test]
fn addresses_a_peer_on_the_same_network_could_have_are_accepted() {
    for local in [
        "127.0.0.1:9000",
        "10.0.0.7:9000",
        "172.16.5.5:9000",
        "192.168.0.2:9000",
        "169.254.10.20:9000",
        "[fe80::1]:9000",
        "[fd00::1]:9000",
    ] {
        assert!(
            DeviceLinkJoinInvitation::parse_url(&invitation_with_local(local)).is_ok(),
            "{local} is reachable on a local network and must be accepted"
        );
    }
}

// @scenario: device_management :: a malformed rendezvous is refused
#[test]
fn a_rendezvous_that_is_not_a_socket_address_is_refused() {
    for malformed in [
        "not-an-address",
        "192.168.1.1",
        "192.168.1.1:not-a-port",
        "192.168.1.1:99999",
    ] {
        assert!(
            DeviceLinkJoinInvitation::parse_url(&invitation_with_local(malformed)).is_err(),
            "{malformed:?} must be refused"
        );
    }
}

// @scenario: device_management :: an empty local param reads as absent
#[test]
fn an_empty_local_param_means_no_local_rendezvous() {
    // Every invitation param treats an empty value as absent, so this
    // falls back to the relay rather than producing a bogus address.
    let parsed = DeviceLinkJoinInvitation::parse_url(&invitation_with_local(""))
        .expect("an empty local param is not malformed");
    assert_eq!(parsed.local_rendezvous, None);
}
