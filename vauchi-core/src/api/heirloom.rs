// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Paper heirloom: a printable, human-readable record of the contact book
//! (private#363).
//!
//! This is a deliberate plaintext export that leaves the encryption
//! envelope. It carries human-facing content only: no keys, fingerprints,
//! contact or place ids, and no coordinates.

use base64::Engine;

/// Avatars are WebP <= 32 KB by construction (ADR-042); anything larger is
/// not one of ours and is left out rather than embedded.
const MAX_AVATAR_BYTES: usize = 32 * 1024;

/// The contact book as it goes on paper.
#[derive(Debug, Clone, PartialEq)]
pub struct HeirloomBook {
    pub owner_name: String,
    pub contacts: Vec<HeirloomContact>,
}

/// One contact's page.
#[derive(Debug, Clone, PartialEq)]
pub struct HeirloomContact {
    pub name: String,
    pub fields: Vec<HeirloomField>,
    /// Unix seconds of the in-person exchange; `None` for imported contacts.
    pub exchanged_at: Option<u64>,
    /// The owner's name for where they met, never the coordinates.
    pub place: Option<String>,
    pub avatar_webp: Option<Vec<u8>>,
}

/// A card field as label and value.
#[derive(Debug, Clone, PartialEq)]
pub struct HeirloomField {
    pub label: String,
    pub value: String,
}

/// Localised wording the caller supplies (ADR-038: Core does not hardcode
/// user-visible strings).
#[derive(Debug, Clone, PartialEq)]
pub struct HeirloomText {
    /// Heading prefix, followed by the owner's name: "Contacts of".
    pub document_title: String,
    /// Prefix of the exchange date: "Exchanged on".
    pub exchanged_on: String,
    /// Joins the date and the place name: "at".
    pub met_at: String,
}

const STYLE: &str = "body{font-family:Georgia,serif;margin:2em;color:#111}\
h1{font-size:1.6em}\
section{break-inside:avoid;page-break-inside:avoid;margin:1.5em 0;\
padding-top:1em;border-top:1px solid #999}\
img.avatar{width:96px;height:96px;object-fit:cover;float:right}\
dt{font-weight:bold}dd{margin:0 0 .4em 1em}\
@media print{section{page-break-after:always;border:0}}";

/// Renders the book as one self-contained HTML document. The output is a
/// pure function of its inputs (no clock, no locale lookups) so the same
/// book always yields the same bytes.
pub fn render_heirloom_html(book: &HeirloomBook, text: &HeirloomText) -> String {
    let heading = format!(
        "{} {}",
        escape(&text.document_title),
        escape(&book.owner_name)
    );
    let mut out = String::new();
    out.push_str("<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!("<title>{heading}</title>\n"));
    out.push_str(&format!("<style>{STYLE}</style>\n</head>\n<body>\n"));
    out.push_str(&format!("<h1>{heading}</h1>\n"));
    for contact in &book.contacts {
        render_contact(&mut out, contact, text);
    }
    out.push_str("</body>\n</html>\n");
    out
}

fn render_contact(out: &mut String, contact: &HeirloomContact, text: &HeirloomText) {
    out.push_str("<section>\n");
    if let Some(avatar) = contact.avatar_webp.as_deref().filter(|a| is_webp(a)) {
        let encoded = base64::engine::general_purpose::STANDARD.encode(avatar);
        out.push_str(&format!(
            "<img class=\"avatar\" alt=\"\" src=\"data:image/webp;base64,{encoded}\">\n"
        ));
    }
    out.push_str(&format!("<h2>{}</h2>\n", escape(&contact.name)));
    if let Some(met) = met_line(contact, text) {
        out.push_str(&format!("<p class=\"met\">{met}</p>\n"));
    }
    if !contact.fields.is_empty() {
        out.push_str("<dl>");
        for field in &contact.fields {
            out.push_str(&format!(
                "<dt>{}</dt><dd>{}</dd>",
                escape(&field.label),
                escape(&field.value)
            ));
        }
        out.push_str("</dl>\n");
    }
    out.push_str("</section>\n");
}

fn met_line(contact: &HeirloomContact, text: &HeirloomText) -> Option<String> {
    let date = contact
        .exchanged_at
        .map(|secs| format!("{} {}", escape(&text.exchanged_on), calendar_day(secs)));
    let place = contact
        .place
        .as_deref()
        .map(|p| format!("{} {}", escape(&text.met_at), escape(p)));
    match (date, place) {
        (Some(d), Some(p)) => Some(format!("{d} {p}")),
        (Some(d), None) => Some(d),
        (None, Some(p)) => Some(p),
        (None, None) => None,
    }
}

/// The file's content decides what it is, not the field it came from
/// (DC-03): only a RIFF/WEBP container within the ADR-042 ceiling is
/// embedded under an image/webp MIME type.
fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12
        && bytes.len() <= MAX_AVATAR_BYTES
        && &bytes[0..4] == b"RIFF"
        && &bytes[8..12] == b"WEBP"
}

fn escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

/// `YYYY-MM-DD` of a Unix timestamp in UTC, by Howard Hinnant's
/// days-to-civil algorithm, so no date crate is needed for one format.
fn calendar_day(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}
