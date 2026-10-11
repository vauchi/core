// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! The https form an exchange QR displays (vauchi/private#277): a phone
//! camera without the app opens vauchi.app/x instead of showing raw base64.
//! The payload rides in the URL fragment, which browsers never send to a
//! server.

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};

const QR_URL_PREFIX: &str = "https://vauchi.app/x#q=";

/// Well above the largest exchange QR (a 255-byte name plus a relay URL),
/// so oversized scans are refused before decoding.
const MAX_SCANNED_QR_LEN: usize = 4096;

pub(crate) fn encode_qr_url(payload: &[u8]) -> String {
    format!("{QR_URL_PREFIX}{}", URL_SAFE_NO_PAD.encode(payload))
}

/// Decodes a scanned QR in either form: the landing URL, or the raw
/// standard base64 that apps from before #277 display.
pub(crate) fn decode_scanned_qr(data: &str) -> Option<Vec<u8>> {
    let data = data.trim();
    if data.len() > MAX_SCANNED_QR_LEN {
        return None;
    }
    match data.strip_prefix(QR_URL_PREFIX) {
        Some(payload) => URL_SAFE_NO_PAD.decode(payload).ok(),
        None => STANDARD.decode(data).ok(),
    }
}
