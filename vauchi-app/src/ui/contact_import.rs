// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared bits of the vCard import affordance.
//!
//! Lived in `more.rs` while the More menu was its only entry point. The
//! Contacts screen offers it too now, and the More menu is being retired.

/// MIME types accepted for vCard import. Frontends may filter the
/// native picker to these; on platforms where the OS picker doesn't
/// filter by MIME (older Android variants), the frontend defaults to
/// `*/*` — the parser rejects non-vCard payloads anyway.
pub(crate) fn vcf_mime_types() -> Vec<String> {
    vec![
        "text/vcard".into(),
        "text/x-vcard".into(),
        "text/directory".into(),
    ]
}
