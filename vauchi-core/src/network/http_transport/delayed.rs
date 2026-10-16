// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Asynchronous OHTTP (vauchi/private#628, design
//! `2026-10-11-outer-relay-delayed-forwarding-design`).
//!
//! A request is posted under a ticket the client chose; the outer relay
//! forwards it after a random delay and holds the encapsulated answer until
//! the client collects it on a later poll. The gateway then sees the fetch
//! long after the client's connection, which breaks the timing link an ISP
//! and the gateway could otherwise draw between an IP and its tokens.
//!
//! The response context lives in memory only (D-F4): `ohttp` exposes no way
//! to store it. Losing it costs a re-post, never mail, because a fetch only
//! peeks and an acknowledgement converges like any duplicate.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use vauchi_protocol::v2::{FetchedBlob, V2AckRequest, V2FetchRequest, V2Response};

use super::{HttpTransport, response_error};
use crate::network::error::NetworkError;
use crate::network::ohttp_client::ResponseDecryptor;
use crate::version::APP_COMPAT_VERSION;

/// Header carrying the ticket of an asynchronous request.
const TICKET_HEADER: &str = "vauchi-ticket";

/// Header in which the outer relay advertises its delayed path and its
/// delay window W (design D-F6, amended 2026-10-16: no relay directory yet).
const DELAY_WINDOW_HEADER: &str = "vauchi-ohttp-delay-window";

/// The longest window a client accepts; a relay advertising more is treated
/// as not advertising, so it cannot make a waiting sync hang on.
const MAX_DELAY_WINDOW_SECS: u64 = 3600;

/// What the outer relay last said about its delayed path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelayWindowAdvert {
    /// It forwards delayed requests within this window.
    Advertised(Duration),
    /// An answer without a valid window, or a refused delayed post.
    Withdrawn,
}

/// W from the header's value: decimal seconds from 1 to 3600, nothing else
/// (the header is unauthenticated input, DC-01).
pub fn parse_delay_window(value: &str) -> Option<Duration> {
    if value.is_empty() || value.len() > 4 || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let seconds: u64 = value.parse().ok()?;
    (1..=MAX_DELAY_WINDOW_SECS)
        .contains(&seconds)
        .then(|| Duration::from_secs(seconds))
}

/// Polls after which a ticket that is still pending counts as lost. The
/// outer relay drops an answer 2W after forwarding (3W after the post, at
/// most 45 minutes with W = 900 s); at about a poll a minute, 60 polls
/// outlast that, and the cap keeps a relay that answers `202` forever from
/// holding a ticket, and with it every fetch, indefinitely.
pub const MAX_PENDING_POLLS: u32 = 60;

/// A posted request whose answer has not been collected yet.
pub struct PendingOhttp {
    ticket: String,
    decryptor: ResponseDecryptor,
    polls: u32,
}

impl PendingOhttp {
    /// The 128-bit ticket, as 32 hex characters.
    pub fn ticket(&self) -> &str {
        &self.ticket
    }
}

impl std::fmt::Debug for PendingOhttp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingOhttp").finish_non_exhaustive()
    }
}

/// What collecting a ticket found.
#[derive(Debug)]
pub enum Collected<T> {
    Ready(T),
    /// Not forwarded yet, or the outer relay could not be reached: try again
    /// on a later poll with the returned handle.
    Pending(PendingOhttp),
    /// The outer relay no longer holds an answer (expired, restarted, or the
    /// gateway failed): post the request again.
    Lost,
}

/// A pending fetch and the mailbox tokens it covers.
#[derive(Debug)]
pub(crate) struct PendingFetch {
    pub(crate) tokens: Vec<String>,
    pub(crate) pending: PendingOhttp,
}

/// Requests posted in earlier sync cycles. The HTTP adapter is rebuilt
/// every cycle, so this outlives it on the long-lived `Vauchi`.
#[derive(Debug, Default)]
pub struct DelayedRequests {
    pub(crate) fetches: Vec<PendingFetch>,
    pub(crate) acks: Vec<PendingOhttp>,
    pub(crate) answered_fetches: u64,
}

impl DelayedRequests {
    pub fn shared() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self::default()))
    }

    /// Fetch answers collected so far.
    pub fn answered_fetches(&self) -> u64 {
        self.answered_fetches
    }
}

impl HttpTransport {
    /// What the outer relay last said about its delayed path, if any of its
    /// answers has been seen.
    pub fn delay_window_advert(&self) -> Option<DelayWindowAdvert> {
        *self
            .delay_window_advert
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Notes the window a synchronous OHTTP answer advertises, or its absence.
    pub(super) fn note_delay_window<B>(&self, resp: &ureq::http::Response<B>) {
        let advert = resp
            .headers()
            .get(DELAY_WINDOW_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_delay_window)
            .map_or(DelayWindowAdvert::Withdrawn, DelayWindowAdvert::Advertised);
        self.record_delay_window(advert);
    }

    fn record_delay_window(&self, advert: DelayWindowAdvert) {
        *self
            .delay_window_advert
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(advert);
    }

    /// Posts a fetch for `mailbox_tokens`; its blobs arrive on a later
    /// [`Self::collect_delayed_fetch`].
    pub fn post_delayed_fetch(
        &self,
        mailbox_tokens: &[String],
    ) -> Result<PendingOhttp, NetworkError> {
        let request = V2FetchRequest {
            mailbox_tokens: mailbox_tokens.to_vec(),
        };
        self.post_delayed("fetch", &request)
    }

    /// Collects a delayed fetch: the blobs and whether the relay truncated
    /// the page.
    pub fn collect_delayed_fetch(
        &self,
        pending: PendingOhttp,
    ) -> Result<Collected<(Vec<FetchedBlob>, bool)>, NetworkError> {
        Ok(match self.collect_delayed(pending)? {
            Collected::Ready(resp) if resp.status == "ok" => {
                Collected::Ready((resp.blobs.unwrap_or_default(), resp.truncated))
            }
            Collected::Ready(resp) => {
                return Err(response_error("fetch", &resp.error.unwrap_or_default()));
            }
            Collected::Pending(pending) => Collected::Pending(pending),
            Collected::Lost => Collected::Lost,
        })
    }

    /// Posts an acknowledgement that removes `blob_id` once forwarded.
    pub fn post_delayed_ack(
        &self,
        recipient_id: &str,
        blob_id: &str,
    ) -> Result<PendingOhttp, NetworkError> {
        let request = V2AckRequest {
            recipient_id: recipient_id.to_string(),
            blob_id: blob_id.to_string(),
        };
        self.post_delayed("ack", &request)
    }

    /// Collects a delayed acknowledgement: whether the relay removed a blob.
    pub fn collect_delayed_ack(
        &self,
        pending: PendingOhttp,
    ) -> Result<Collected<bool>, NetworkError> {
        Ok(match self.collect_delayed(pending)? {
            Collected::Ready(resp) if resp.status == "ok" => {
                Collected::Ready(resp.acknowledged.unwrap_or(false))
            }
            Collected::Ready(resp) => {
                return Err(response_error("ack", &resp.error.unwrap_or_default()));
            }
            Collected::Pending(pending) => Collected::Pending(pending),
            Collected::Lost => Collected::Lost,
        })
    }

    fn post_delayed<Req: Serialize>(
        &self,
        action: &str,
        body: &Req,
    ) -> Result<PendingOhttp, NetworkError> {
        let padded_payload = Self::build_padded_ohttp_payload(action, body)?;
        let (encrypted, decryptor) = {
            let guard = self.ohttp_reader();
            let ohttp = guard.as_ref().ok_or(NetworkError::NotConnected)?;
            ohttp.encapsulate(&padded_payload)?
        };
        let mut ticket_bytes = [0u8; 16];
        crate::crypto::random_fill(&mut ticket_bytes);
        let ticket = hex::encode(ticket_bytes);

        let resp = self
            .get_agent()?
            .post(&format!("{}/v2/ohttp-async", self.config.relay_url))
            .header("X-App-Compat-Version", &APP_COMPAT_VERSION.to_string())
            .header(TICKET_HEADER, &ticket)
            .content_type("message/ohttp-req")
            .send(encrypted)
            .map_err(Self::map_ureq_error)?;
        if resp.status().as_u16() == 404 {
            self.record_delay_window(DelayWindowAdvert::Withdrawn);
        }
        self.check_response_status(&resp)?;
        Ok(PendingOhttp {
            ticket,
            decryptor,
            polls: 0,
        })
    }

    fn collect_delayed(
        &self,
        pending: PendingOhttp,
    ) -> Result<Collected<V2Response>, NetworkError> {
        let url = format!(
            "{}/v2/ohttp-async/{}",
            self.config.relay_url, pending.ticket
        );
        let Ok(resp) = self.get_agent()?.get(&url).call() else {
            return Ok(still_pending(pending));
        };
        match resp.status().as_u16() {
            200 => {}
            202 | 429 => return Ok(still_pending(pending)),
            _ => return Ok(Collected::Lost),
        }
        let encrypted = resp
            .into_body()
            .read_to_vec()
            .map_err(|e| NetworkError::ConnectionFailed(e.to_string()))?;
        let padded = pending.decryptor.decapsulate(&encrypted)?;
        Ok(Collected::Ready(Self::parse_padded_ohttp_response(
            &padded,
        )?))
    }
}

fn still_pending<T>(mut pending: PendingOhttp) -> Collected<T> {
    pending.polls += 1;
    if pending.polls > MAX_PENDING_POLLS {
        return Collected::Lost;
    }
    Collected::Pending(pending)
}
