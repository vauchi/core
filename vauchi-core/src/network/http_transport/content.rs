// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CDN content through the relay's OHTTP `content` action
//! (vauchi/private#733 D8, D11; ADR-037 addendum 2026-10-22). Only through
//! OHTTP: fetching a language names it, so it never goes direct, whatever
//! `allow_direct` says. The caller still checks the signed manifest and each
//! file's checksum; the relay can withhold a file, not alter one unseen.

use base64::Engine as _;
use serde::Deserialize;

use super::HttpTransport;
use crate::network::NetworkError;

#[derive(Deserialize)]
struct ContentAnswer {
    status: String,
    content_b64: Option<String>,
}

impl HttpTransport {
    /// The content file at `path` (such as `locales/pt.json`), from the
    /// relay over OHTTP.
    pub fn fetch_content(&self, path: &str) -> Result<Vec<u8>, NetworkError> {
        let answer: ContentAnswer = self
            .post_via_ohttp_refreshing_stale_key("content", &serde_json::json!({ "path": path }))?;
        match (answer.status.as_str(), answer.content_b64) {
            ("ok", Some(body)) => base64::engine::general_purpose::STANDARD
                .decode(body)
                .map_err(|_| NetworkError::InvalidMessage("content is not base64".into())),
            _ => Err(NetworkError::RelayRejected("content unavailable".into())),
        }
    }
}
