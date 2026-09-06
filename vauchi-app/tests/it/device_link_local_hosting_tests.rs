// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Hosting a device-link ceremony on this device (ADR-070 Phase 1).
//!
//! The socket exists for exactly as long as a scanned QR could still be
//! acted on: bound on entry to the linking screen, gone on the way out.

#![cfg(all(feature = "network-http", feature = "storage"))]

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

use vauchi_app::ui::{AppEngine, AppScreen};
use vauchi_core::api::Vauchi;

fn engine_with_identity() -> AppEngine {
    let mut vauchi = Vauchi::in_memory().expect("in-memory vauchi");
    vauchi.create_identity("Alice").expect("identity");
    AppEngine::new(vauchi)
}

/// The address a shell would report for this device.
const REPORTED: &str = "192.168.1.50";

// @scenario: device_management :: a reported address makes the device host locally
#[test]
fn the_invitation_advertises_the_socket_when_the_shell_reports_an_address() {
    let mut engine = engine_with_identity();
    engine.set_local_network_address(Some(REPORTED.to_string()));

    let _ = engine.navigate_to(AppScreen::DeviceLinking);

    let advertised = engine
        .device_link_local_rendezvous()
        .expect("a hosting device must advertise where it listens");
    let (host, port) = advertised
        .rsplit_once(':')
        .expect("advertised as host:port");

    assert_eq!(host, REPORTED, "the shell's address is what a joiner sees");
    assert!(
        port.parse::<u16>().is_ok_and(|p| p != 0),
        "the OS-assigned port must be real, got {port}"
    );
}

// @scenario: device_management :: no reported address keeps the ceremony on the relay
#[test]
fn no_reported_address_means_no_socket_and_no_advertisement() {
    // A listener nobody can be told about is attack surface with no
    // benefit, so not knowing our address is a reason not to open one.
    let mut engine = engine_with_identity();

    let _ = engine.navigate_to(AppScreen::DeviceLinking);

    assert_eq!(
        engine.device_link_local_rendezvous(),
        None,
        "without an address the ceremony stays on the relay"
    );
}

// @scenario: device_management :: leaving the screen closes the socket
#[test]
fn navigating_away_closes_the_socket_not_just_the_session() {
    let mut engine = engine_with_identity();
    engine.set_local_network_address(Some(REPORTED.to_string()));
    let _ = engine.navigate_to(AppScreen::DeviceLinking);

    let advertised = engine
        .device_link_local_rendezvous()
        .expect("hosting on entry");
    let port: u16 = advertised
        .rsplit_once(':')
        .and_then(|(_, p)| p.parse().ok())
        .expect("a real port");
    // The bind is on every interface, so loopback reaches the same socket.
    let served = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    assert!(
        TcpStream::connect_timeout(&served, Duration::from_millis(500)).is_ok(),
        "the socket must be live while the ceremony is on screen"
    );

    let _ = engine.navigate_to(AppScreen::Settings);

    assert_eq!(
        engine.device_link_local_rendezvous(),
        None,
        "the session is gone"
    );

    // And the socket with it — a listener outliving its ceremony is
    // exactly the surface ADR-070 binds it narrowly to avoid. Polled for
    // rather than slept on (CC-06); the stop is one accept-poll away.
    let mut closed = false;
    for _ in 0..200 {
        // Refusal specifically, never merely "an error": a timeout is not
        // evidence the socket is gone, and accepting one would let a still-
        // bound listener read as closed.
        let refused = TcpStream::connect_timeout(&served, Duration::from_millis(100))
            .err()
            .is_some_and(|e| e.kind() == std::io::ErrorKind::ConnectionRefused);
        if refused {
            closed = true;
            break;
        }
        std::thread::yield_now();
    }
    assert!(
        closed,
        "the socket must not outlive the ceremony that justified it"
    );
}
