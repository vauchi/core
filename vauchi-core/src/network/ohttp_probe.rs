// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! One encapsulated round trip through an OHTTP relay and gateway.
//!
//! `/health` stayed 200 on both the relay and the OHTTP host while every
//! encapsulated POST returned 502, so only a real encapsulated request shows
//! that the forward hop works (problems/2026-05-25-relay-ohttp-forward-hop-502,
//! goal G3). The probe sends a `fetch` for a fresh random mailbox token: the
//! relay only reads for it and answers with no blobs, so the probe needs no
//! identity and changes no state.

use std::fmt;

use super::http_transport::{HttpTransport, HttpTransportConfig};
use super::ohttp_client::OhttpClient;
use super::pinning::PinnedCertificate;
use super::transport::ProxyConfig;

/// The step of the round trip that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OhttpProbeStep {
    /// `GET /v2/ohttp-key` on the OHTTP relay.
    KeyFetch,
    /// The fetched bytes are not a usable OHTTP key config.
    KeyConfig,
    /// `POST /v2/ohttp` — forward hop, gateway decapsulation, relay dispatch
    /// and the encapsulated answer.
    EncapsulatedRequest,
}

impl fmt::Display for OhttpProbeStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::KeyFetch => "key fetch",
            Self::KeyConfig => "key config",
            Self::EncapsulatedRequest => "encapsulated request",
        })
    }
}

/// Why the probe failed, and at which step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OhttpProbeFailure {
    pub step: OhttpProbeStep,
    pub detail: String,
}

impl fmt::Display for OhttpProbeFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} failed: {}", self.step, self.detail)
    }
}

impl std::error::Error for OhttpProbeFailure {}

/// Sends one encapsulated `fetch` through `ohttp_relay_url` and checks the
/// decapsulated answer. `pinned_certs` are the OHTTP host's SPKI pins
/// (`RelayConfig::ohttp_pinned_certs`); empty disables pinning.
pub fn probe_ohttp_forward_hop(
    ohttp_relay_url: &str,
    pinned_certs: Vec<PinnedCertificate>,
    timeout_ms: u64,
) -> Result<(), OhttpProbeFailure> {
    let failed = |step| {
        move |error: super::NetworkError| OhttpProbeFailure {
            step,
            detail: error.to_string(),
        }
    };

    let mut transport = HttpTransport::new(HttpTransportConfig {
        relay_url: ohttp_relay_url.trim_end_matches('/').to_string(),
        timeout_ms,
        proxy: ProxyConfig::None,
        allow_direct: false,
        pinned_certs,
    });

    let key = transport
        .fetch_ohttp_key()
        .map_err(failed(OhttpProbeStep::KeyFetch))?;
    let client = OhttpClient::new(key).map_err(failed(OhttpProbeStep::KeyConfig))?;
    transport.set_ohttp(client);

    let token: [u8; 32] = crate::crypto::random_bytes();
    transport
        .fetch(&[hex::encode(token)])
        .map(|_| ())
        .map_err(failed(OhttpProbeStep::EncapsulatedRequest))
}
