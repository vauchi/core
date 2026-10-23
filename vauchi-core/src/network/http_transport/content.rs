// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CDN content through the relay's OHTTP `content` action
//! (vauchi/private#733 D8, D11; ADR-037 addendum 2026-10-22). Only through
//! OHTTP: fetching a language names it, so it never goes direct, whatever
//! `allow_direct` says. The caller still checks the signed manifest and each
//! file's checksum; the relay can withhold a file, not alter one unseen.

use std::io::Read as _;

use base64::Engine as _;
use serde::Deserialize;

use super::HttpTransport;
use crate::network::NetworkError;

/// The largest content file Core inflates, as the relay's content proxy
/// takes at most 5 MiB from the CDN; a small gzip answer could otherwise
/// inflate without bound.
const MAX_CONTENT_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Deserialize)]
struct ContentAnswer {
    status: String,
    content_b64: Option<String>,
    encoding: Option<String>,
}

impl HttpTransport {
    /// The content file at `path` (such as `locales/pt.json`), from the
    /// relay over OHTTP. Asked for gzipped: a locale file is larger than
    /// the OHTTP relay passes back (vauchi/private#733 D14). An older relay
    /// answers plain.
    pub fn fetch_content(&self, path: &str) -> Result<Vec<u8>, NetworkError> {
        let answer: ContentAnswer = self.post_via_ohttp_refreshing_stale_key(
            "content",
            &serde_json::json!({ "path": path, "encoding": "gzip" }),
        )?;
        let ("ok", Some(body)) = (answer.status.as_str(), answer.content_b64) else {
            return Err(NetworkError::RelayRejected("content unavailable".into()));
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(body)
            .map_err(|_| NetworkError::InvalidMessage("content is not base64".into()))?;
        match answer.encoding.as_deref() {
            None => Ok(bytes),
            Some("gzip") => gunzip(&bytes),
            Some(_) => Err(NetworkError::InvalidMessage(
                "content in an unknown encoding".into(),
            )),
        }
    }
}

fn gunzip(compressed: &[u8]) -> Result<Vec<u8>, NetworkError> {
    let mut content = Vec::new();
    flate2::read::GzDecoder::new(compressed)
        .take(MAX_CONTENT_BYTES + 1)
        .read_to_end(&mut content)
        .map_err(|_| NetworkError::InvalidMessage("content is not gzip".into()))?;
    if content.is_empty() || content.len() as u64 > MAX_CONTENT_BYTES {
        return Err(NetworkError::InvalidMessage(
            "compressed content out of bounds".into(),
        ));
    }
    Ok(content)
}
