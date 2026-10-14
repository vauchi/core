// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Formats affected screen IDs for the C ABI event callback.

/// A JSON array of screen IDs, as the event callback receives it.
pub fn screens_json(ids: &[String]) -> String {
    let inner: Vec<String> = ids.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", inner.join(","))
}
