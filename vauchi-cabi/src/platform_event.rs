// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Maps `VauchiEvent` to affected screen IDs for the C ABI event callback.

use vauchi_core::api::VauchiEvent;

/// Returns a JSON array of screen IDs affected by the given event.
///
/// Example: `["contacts","contact_detail"]`
pub fn affected_screens_json(event: &VauchiEvent) -> String {
    let ids = vauchi_app::ui::affected_screens(event);
    if ids.is_empty() {
        return "[]".to_string();
    }
    let inner: Vec<String> = ids.iter().map(|s| format!("\"{s}\"")).collect();
    format!("[{}]", inner.join(","))
}
