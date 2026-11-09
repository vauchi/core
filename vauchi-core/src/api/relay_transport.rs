// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

//! The fail-closed application-relay transport, built from the relay
//! configuration alone, so a run without a profile (a locked phone,
//! vauchi/private#1093) builds the same one a profile does.

use std::sync::Arc;

use crate::api::config::{RelayConfig, http_origin};
use crate::clock::Clock;
use crate::network::ohttp_key_trust::HeldOhttpKey;
use crate::network::request_pacer::RequestPacer;
use crate::network::{HttpTransport, HttpTransportConfig, OhttpClient, PinnedCertificate};

/// OHTTP-relay base URL for `/v2/ohttp` and the `/v2/ohttp-key-signed`
/// bootstrap (problem 2026-05-25-relay-ohttp-forward-hop-502).
pub(crate) fn http_relay_url(relay: &RelayConfig) -> String {
    crate::api::config::ohttp_endpoint(&relay.server_url, relay.ohttp_relay_url.as_deref())
}

pub(crate) fn distinct_ohttp_route(relay: &RelayConfig) -> Option<String> {
    relay.ohttp_route_for(&relay.server_url)
}

pub(crate) fn action_ohttp_route(
    relay: &RelayConfig,
    application_relay_url: &str,
) -> Option<String> {
    let configured_origin = http_origin(&relay.server_url)?;
    let requested_origin = http_origin(application_relay_url)?;
    (configured_origin == requested_origin)
        .then(|| distinct_ohttp_route(relay))
        .flatten()
}

/// Pin set for a transport targeting `relay_url`.
///
/// When `relay_url` is the distinct OHTTP-relay host (`ohttp.vauchi.app`
/// in production — a separate entity with its own TLS key per ADR-037),
/// pin ITS SPKI (`ohttp_pinned_certs`). Otherwise — the OHTTP endpoint
/// shares the data-relay host (self-hosted / local / e2e), or the target
/// is the data relay itself — use `relay_pins` (the data-relay set the
/// caller would otherwise apply).
///
/// Fixes 2026-05-25-relay-ohttp-forward-hop-502: pinning the relay's
/// SPKI against `ohttp.vauchi.app` made every production sync fail
/// `certificate pin verification failed`.
pub(crate) fn ohttp_endpoint_pins(
    relay: &RelayConfig,
    relay_url: &str,
    relay_pins: Vec<PinnedCertificate>,
) -> Vec<PinnedCertificate> {
    let ohttp = http_relay_url(relay);
    if relay_url == ohttp && ohttp != relay.server_url {
        relay.ohttp_pinned_certs.clone()
    } else {
        relay_pins
    }
}

/// Whether the gateway still accepts a key held for `window` in the
/// `current` window: the one before, its own, or the one after.
pub(crate) fn gateway_still_holds(window: u64, current: u64) -> bool {
    window.saturating_add(1) >= current && window <= current.saturating_add(1)
}

/// The OHTTP key a relay transport is given.
pub(crate) enum TransportOhttpKey {
    /// An anchored relay's held signed key, while the gateway still
    /// accepts it.
    Signed(HeldOhttpKey),
    /// A relay without an anchor: an in-memory key (testing).
    Unanchored(OhttpClient),
    None,
}

/// A fail-closed `HttpTransport` for application relay actions.
///
/// The outer endpoint is derived internally so callers cannot accidentally
/// send OHTTP straight to the application relay. Construction performs no
/// network I/O. Without a key, action methods return their fail-closed
/// error before sending a request.
pub(crate) fn relay_transport(
    relay: &RelayConfig,
    application_relay_url: &str,
    timeout_ms: u64,
    pacer: Arc<RequestPacer>,
    key: TransportOhttpKey,
    clock: Arc<dyn Clock>,
) -> HttpTransport {
    let route = action_ohttp_route(relay, application_relay_url);
    let route_valid = route.is_some();
    let relay_url = route.unwrap_or_else(|| http_relay_url(relay));
    let pinned_certs = ohttp_endpoint_pins(relay, &relay_url, relay.pinned_certs.clone());
    let mut transport = HttpTransport::new(HttpTransportConfig {
        relay_url,
        timeout_ms,
        proxy: relay.proxy.clone(),
        allow_direct: false,
        pinned_certs,
    })
    .with_pacer(pacer);

    if route_valid {
        match (relay.ohttp_trust_anchor(), key) {
            // An anchored relay's transport carries its held signed key or
            // none, and refetches only signed keys (#288).
            (Some(anchor), TransportOhttpKey::Signed(held)) => {
                #[allow(clippy::let_underscore_must_use)]
                let _ = transport.set_signed_ohttp(anchor, held, clock);
            }
            (None, TransportOhttpKey::Unanchored(client)) => transport.set_ohttp(client),
            _ => {}
        }
    }

    transport
}
