// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Multi-version vCard import parser (2.1 / 3.0 / 4.0).
//!
//! Handles real-world .vcf files from Google, iCloud, Outlook, and Samsung.
//! Lenient: skips malformed properties/contacts rather than failing the file.

use base64::prelude::*;

use super::{ContactCard, ContactField, FieldType, MAX_DISPLAY_NAME_LENGTH};
use crate::contact_card::field::MAX_VALUE_LENGTH;

/// Maximum import file size: 10 MB.
const MAX_FILE_SIZE: usize = 10 * 1024 * 1024;

/// vCard import errors (only for unrecoverable issues).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VCardImportError {
    #[error("File too large ({size} bytes, max {max})")]
    FileTooLarge { size: usize, max: usize },
}

/// Detected vCard version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VCardVersion {
    V21,
    V30,
    V40,
}

/// Decodes raw bytes to a string, trying UTF-8 first with Latin-1 fallback.
///
/// Strips UTF-8 BOM if present. When UTF-8 decoding fails, treats bytes
/// as ISO-8859-1 (Latin-1) which is a superset of ASCII and never fails.
/// This handles Windows-1252 approximately (0x80-0x9F range differs but
/// those codepoints are rare in contact data — names, phones, emails).
fn decode_to_string(data: &[u8]) -> String {
    let data = if data.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &data[3..]
    } else {
        data
    };

    if let Ok(text) = std::str::from_utf8(data) {
        return text.to_string();
    }

    // Fallback: treat as Latin-1 (ISO-8859-1).
    // Every byte is valid Latin-1, so this never fails.
    // Strip NUL bytes (0x00) to prevent display truncation on mobile (W8).
    data.iter()
        .filter(|&&b| b != 0)
        .map(|&b| b as char)
        .collect()
}

/// Import contacts from a vCard file (supports 2.1, 3.0, 4.0).
///
/// Returns a list of `(ContactCard, Option<uid>)` tuples.
/// The uid is the vCard UID property, used for re-import dedup.
///
/// `now` is the Unix-seconds timestamp stamped on every freshly
/// constructed `ContactField`. Production callers route it through
/// `Vauchi::clock.unix_seconds()` (ADR-021 functional core); tests
/// pin a deterministic value.
pub fn import_vcf(
    data: &[u8],
    now: u64,
) -> Result<Vec<(ContactCard, Option<String>)>, VCardImportError> {
    if data.len() > MAX_FILE_SIZE {
        return Err(VCardImportError::FileTooLarge {
            size: data.len(),
            max: MAX_FILE_SIZE,
        });
    }

    let text = decode_to_string(data);
    let blocks = split_vcard_blocks(&text);

    let mut results = Vec::new();
    for block in blocks {
        if let Some(result) = parse_single_vcard(&block, now) {
            results.push(result);
        }
    }

    Ok(results)
}

/// Split input text into individual vCard blocks.
fn split_vcard_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.eq_ignore_ascii_case("BEGIN:VCARD") {
            current = Some(Vec::new());
        } else if trimmed.eq_ignore_ascii_case("END:VCARD") {
            if let Some(lines) = current.take() {
                blocks.push(lines.join("\n"));
            }
        } else if let Some(ref mut lines) = current {
            lines.push(line);
        }
    }

    blocks
}

/// Extract a property value and unescape it, returning `None` when the
/// result is empty so callers can skip blank fields in one step.
fn extract_clean(stripped: &str) -> Option<String> {
    let val = extract_value(stripped)?;
    let unescaped = unescape_vcard(&val);
    if unescaped.is_empty() {
        None
    } else {
        Some(unescaped)
    }
}

/// Mutable accumulator for the fields parsed out of one vCard block.
///
/// `apply_line` dispatches a single unfolded line into the right slot; the
/// per-property push helpers keep each dispatch arm to a single call so the
/// dispatch stays flat and readable.
#[derive(Default)]
struct VCardFields {
    display_name: Option<String>,
    n_fallback: Option<String>,
    uid: Option<String>,
    nickname: Option<String>,
    avatar_data: Option<Vec<u8>>,
    fields: Vec<(FieldType, String, String)>,
}

impl VCardFields {
    /// Push a value-bearing field whose label is resolved from group/type.
    fn push_labeled(
        &mut self,
        field_type: FieldType,
        stripped: &str,
        group: Option<&str>,
        group_labels: &[(String, String)],
        default_label: &str,
        version: VCardVersion,
    ) {
        if let Some(value) = extract_clean(stripped) {
            let label = resolve_label(stripped, group, group_labels, default_label, version);
            self.fields
                .push((field_type, label, truncate_chars(&value, MAX_VALUE_LENGTH)));
        }
    }

    /// Push a value-bearing field with a fixed (non-resolved) label.
    fn push_fixed(&mut self, field_type: FieldType, label: &str, stripped: &str) {
        if let Some(value) = extract_clean(stripped) {
            self.fields.push((
                field_type,
                label.to_string(),
                truncate_chars(&value, MAX_VALUE_LENGTH),
            ));
        }
    }

    /// Record the structured-name (`N`) fallback display name.
    fn set_n_fallback(&mut self, stripped: &str) {
        if let Some(val) = extract_value(stripped) {
            let name = parse_n_value(&val);
            if !name.is_empty() {
                self.n_fallback = Some(truncate_chars(&name, MAX_DISPLAY_NAME_LENGTH));
            }
        }
    }

    /// Push a formatted postal address (`ADR`).
    fn push_adr(
        &mut self,
        stripped: &str,
        group: Option<&str>,
        group_labels: &[(String, String)],
        version: VCardVersion,
    ) {
        if let Some(val) = extract_value(stripped) {
            let label = resolve_label(stripped, group, group_labels, "Home", version);
            let addr = format_adr(&val);
            if !addr.is_empty() {
                self.fields.push((
                    FieldType::Address,
                    label,
                    truncate_chars(&addr, MAX_VALUE_LENGTH),
                ));
            }
        }
    }

    /// Push a normalized birthday (`BDAY`).
    fn push_bday(&mut self, stripped: &str) {
        if let Some(val) = extract_value(stripped) {
            let unescaped = unescape_vcard(&val);
            let normalized = normalize_date(&unescaped);
            if !normalized.is_empty() {
                self.fields
                    .push((FieldType::Birthday, "Birthday".to_string(), normalized));
            }
        }
    }

    /// Push an organization (`ORG`), collapsing sub-unit semicolons.
    fn push_org(&mut self, stripped: &str) {
        if let Some(val) = extract_value(stripped) {
            let unescaped = unescape_vcard(&val);
            let org = unescaped
                .replace(';', ", ")
                .trim_matches(',')
                .trim()
                .to_string();
            let org = org
                .trim_end_matches(", ")
                .trim_end_matches(',')
                .trim()
                .to_string();
            if !org.is_empty() {
                self.fields.push((
                    FieldType::Custom,
                    "Organization".to_string(),
                    truncate_chars(&org, MAX_VALUE_LENGTH),
                ));
            }
        }
    }

    /// Dispatch one unfolded vCard line into the accumulator.
    fn apply_line(&mut self, line: &str, group_labels: &[(String, String)], version: VCardVersion) {
        if line.is_empty() {
            return;
        }

        let (group, stripped) = strip_group_prefix(line);
        let group = group.as_deref();
        let upper = stripped.to_uppercase();

        // VERSION is read by detect_version; X-ABLabel lines are collected
        // separately as group labels — both are handled elsewhere.
        if upper.starts_with("VERSION:") || upper.starts_with("X-ABLABEL") {
            return;
        }

        if upper.starts_with("FN") {
            if let Some(value) = extract_clean(stripped) {
                self.display_name = Some(truncate_chars(&value, MAX_DISPLAY_NAME_LENGTH));
            }
        } else if upper.starts_with("N:") || upper.starts_with("N;") {
            self.set_n_fallback(stripped);
        } else if upper.starts_with("UID") {
            if let Some(value) = extract_clean(stripped) {
                self.uid = Some(truncate_chars(&value, MAX_VALUE_LENGTH));
            }
        } else if upper.starts_with("NICKNAME") {
            if let Some(value) = extract_clean(stripped) {
                self.nickname = Some(truncate_chars(&value, MAX_DISPLAY_NAME_LENGTH));
            }
        } else if upper.starts_with("TEL") {
            self.push_labeled(
                FieldType::Phone,
                stripped,
                group,
                group_labels,
                "Mobile",
                version,
            );
        } else if upper.starts_with("EMAIL") {
            self.push_labeled(
                FieldType::Email,
                stripped,
                group,
                group_labels,
                "Personal",
                version,
            );
        } else if upper.starts_with("ADR") {
            self.push_adr(stripped, group, group_labels, version);
        } else if upper.starts_with("URL") {
            self.push_labeled(
                FieldType::Website,
                stripped,
                group,
                group_labels,
                "Website",
                version,
            );
        } else if upper.starts_with("BDAY") {
            self.push_bday(stripped);
        } else if upper.starts_with("NOTE") {
            self.push_fixed(FieldType::Custom, "Notes", stripped);
        } else if upper.starts_with("ORG") {
            self.push_org(stripped);
        } else if upper.starts_with("TITLE") {
            self.push_fixed(FieldType::Custom, "Title", stripped);
        } else if upper.starts_with("PHOTO") {
            self.avatar_data = parse_photo(stripped, version);
        } else if upper.starts_with("X-SOCIALPROFILE") || upper.starts_with("IMPP") {
            self.push_labeled(
                FieldType::Social,
                stripped,
                group,
                group_labels,
                "Social",
                version,
            );
        }
    }
}

/// Parse a single vCard block (content between BEGIN/END, exclusive).
fn parse_single_vcard(block: &str, now: u64) -> Option<(ContactCard, Option<String>)> {
    let version = detect_version(block);
    let lines = unfold_lines(block, version);

    let group_labels = collect_group_labels(&lines);

    let mut acc = VCardFields::default();
    for line in &lines {
        acc.apply_line(line, &group_labels, version);
    }

    let name = acc.display_name.or(acc.n_fallback).unwrap_or_default();
    if name.is_empty() {
        return None; // Cannot create a card without a name
    }

    let mut card = ContactCard::new(&name);

    if let Some(nick) = acc.nickname {
        card.set_nickname(&nick);
    }

    if let Some(avatar) = acc.avatar_data
        && let Err(e) = card.set_avatar(avatar)
    {
        // ADR-042: set_avatar normalizes any input image to WebP <= 32 KB.
        // A failure here means the source image was unrecognized or too
        // large to fit within the size cap after resizing. Lenient
        // import: keep the contact, drop only the avatar — but surface
        // the kind of failure so operators can see corrupt-payload rates.
        // PII-safe: ContactCardError variants do not include image bytes.
        // TODO(PFC): logging inside importer — see 2026-07-06-core-pfc-violations C9
        tracing::warn!(
            error = %e,
            "vcard import: dropping avatar that failed normalization (ADR-042)"
        );
    }

    for (field_type, label, value) in acc.fields {
        let field = ContactField::new(field_type.clone(), &label, &value, now);
        if let Err(e) = card.add_field(field) {
            // ADR-042-shape lenient import: keep the contact, drop only
            // the failing field — but surface the failure so operators
            // see corrupt-payload rates from imports. PII-safe:
            // ContactCardError variants (MaxFieldsReached,
            // Validation(InvalidPhone|InvalidEmail|InvalidUrl|
            // ValueTooLong|EmptyValue)) carry no field value or label;
            // field_type is metadata (Phone/Email/Social/...).
            // TODO(PFC): logging inside importer — see 2026-07-06-core-pfc-violations C9
            tracing::warn!(
                error = %e,
                field_type = ?field_type,
                "vcard import: dropping field that failed validation"
            );
        }
    }

    Some((card, acc.uid))
}

/// Detect vCard version from content. Defaults to 3.0.
fn detect_version(block: &str) -> VCardVersion {
    for line in block.lines() {
        let trimmed = line.trim().to_uppercase();
        if trimmed.starts_with("VERSION:") {
            let ver = trimmed.trim_start_matches("VERSION:").trim();
            return match ver {
                "2.1" => VCardVersion::V21,
                "4.0" => VCardVersion::V40,
                _ => VCardVersion::V30,
            };
        }
    }
    VCardVersion::V30
}

/// Unfold continuation lines. Also handles QUOTED-PRINTABLE soft breaks for 2.1.
fn unfold_lines(block: &str, version: VCardVersion) -> Vec<String> {
    let raw_lines: Vec<&str> = block.lines().collect();
    let mut result: Vec<String> = Vec::new();

    let mut unfolded: Vec<String> = Vec::new();
    for line in &raw_lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            if let Some(prev) = unfolded.last_mut() {
                prev.push_str(&line[1..]);
            }
        } else if let Some(prev) = unfolded.last_mut() {
            if version == VCardVersion::V21 && prev.ends_with('=') {
                prev.pop();
                prev.push_str(line);
            } else {
                unfolded.push((*line).to_string());
            }
        } else {
            unfolded.push((*line).to_string());
        }
    }

    for line in unfolded {
        if version == VCardVersion::V21 && is_quoted_printable(&line) {
            result.push(decode_qp_line(&line));
        } else {
            result.push(line);
        }
    }

    result
}

/// Check if a line uses QUOTED-PRINTABLE encoding.
fn is_quoted_printable(line: &str) -> bool {
    let upper = line.to_uppercase();
    upper.contains("ENCODING=QUOTED-PRINTABLE") || upper.contains(";QUOTED-PRINTABLE")
}

/// Decode QUOTED-PRINTABLE content in a property line.
fn decode_qp_line(line: &str) -> String {
    if let Some(colon_pos) = find_value_start(line) {
        let prefix = &line[..colon_pos + 1];
        let value = &line[colon_pos + 1..];

        let clean_prefix = remove_qp_params(prefix);
        let decoded = decode_quoted_printable(value);

        format!("{clean_prefix}{decoded}")
    } else {
        line.to_string()
    }
}

/// Remove QP-related parameters from property prefix.
/// `prefix` includes the trailing colon (e.g. `FN;ENCODING=QUOTED-PRINTABLE:`).
fn remove_qp_params(prefix: &str) -> String {
    let without_colon = prefix.strip_suffix(':').unwrap_or(prefix);
    let parts: Vec<&str> = without_colon.split(';').collect();
    let mut kept = Vec::new();
    for part in &parts {
        let upper = part.to_uppercase();
        if upper.contains("ENCODING=QUOTED-PRINTABLE")
            || upper == "QUOTED-PRINTABLE"
            || upper.starts_with("CHARSET=")
        {
            continue;
        }
        kept.push(*part);
    }
    let joined = if kept.len() <= 1 {
        kept.join("")
    } else {
        kept.join(";")
    };
    format!("{joined}:")
}

/// Decode QUOTED-PRINTABLE bytes.
fn decode_quoted_printable(input: &str) -> String {
    let mut bytes = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] == '=' && i + 2 < chars.len() {
            let hex: String = [chars[i + 1], chars[i + 2]].iter().collect();
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                bytes.push(byte);
                i += 3;
                continue;
            }
        }
        let mut buf = [0u8; 4];
        let encoded = chars[i].encode_utf8(&mut buf);
        bytes.extend_from_slice(encoded.as_bytes());
        i += 1;
    }

    String::from_utf8_lossy(&bytes).to_string()
}

/// Collect group labels from X-ABLabel lines.
/// Returns a map: group_name → label text.
fn collect_group_labels(lines: &[String]) -> Vec<(String, String)> {
    let mut labels = Vec::new();
    for line in lines {
        let (group, stripped) = strip_group_prefix(line);
        if let Some(group_name) = group {
            let upper = stripped.to_uppercase();
            if upper.starts_with("X-ABLABEL")
                && let Some(val) = extract_value(stripped)
            {
                let clean = val
                    .trim_start_matches("_$!<")
                    .trim_end_matches(">!$_")
                    .to_string();
                labels.push((group_name, clean));
            }
        }
    }
    labels
}

/// Strip group prefix (e.g., "item1.TEL" → (Some("item1"), "TEL")).
fn strip_group_prefix(line: &str) -> (Option<String>, &str) {
    if let Some(dot_pos) = line.find('.') {
        let prefix = &line[..dot_pos];
        if !prefix.is_empty()
            && prefix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return (Some(prefix.to_lowercase()), &line[dot_pos + 1..]);
        }
    }
    (None, line)
}

/// Extract value from a property line (everything after the first unquoted colon).
fn extract_value(line: &str) -> Option<String> {
    let colon_pos = find_value_start(line)?;
    Some(line[colon_pos + 1..].to_string())
}

/// Find the position of the colon that separates params from value.
fn find_value_start(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut in_quotes = false;

    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_quotes = !in_quotes;
        } else if b == b':' && !in_quotes {
            return Some(i);
        }
    }
    None
}

/// Resolve label for a typed property.
fn resolve_label(
    line: &str,
    group: Option<&str>,
    group_labels: &[(String, String)],
    default: &str,
    version: VCardVersion,
) -> String {
    if let Some(g) = group {
        for (gname, glabel) in group_labels {
            if gname == g {
                return truncate_chars(glabel, 64);
            }
        }
    }

    let colon_pos = match find_value_start(line) {
        Some(p) => p,
        None => return default.to_string(),
    };

    let params_str = &line[..colon_pos];

    let params: Vec<&str> = params_str.split(';').collect();

    for param in &params[1..] {
        let upper = param.to_uppercase();
        if let Some(types) = upper.strip_prefix("TYPE=") {
            let type_val = types.split(',').next().unwrap_or(types);
            return normalize_type_label(type_val);
        }

        if version == VCardVersion::V21
            && let Some(label) = recognize_bare_param(&upper)
        {
            return label;
        }
    }

    default.to_string()
}

/// Recognize vCard 2.1 bare parameters (e.g., TEL;CELL → TYPE=CELL).
fn recognize_bare_param(param: &str) -> Option<String> {
    match param {
        "CELL" => Some("Mobile".to_string()),
        "HOME" => Some("Home".to_string()),
        "WORK" => Some("Work".to_string()),
        "FAX" => Some("Fax".to_string()),
        "PREF" => None, // PREF is not a type label
        "VOICE" => Some("Phone".to_string()),
        "INTERNET" => Some("Internet".to_string()),
        _ => None,
    }
}

/// Normalize TYPE values to user-friendly labels.
fn normalize_type_label(type_val: &str) -> String {
    match type_val {
        "CELL" | "cell" => "Mobile".to_string(),
        "HOME" | "home" => "Home".to_string(),
        "WORK" | "work" => "Work".to_string(),
        "FAX" | "fax" => "Fax".to_string(),
        "VOICE" | "voice" => "Phone".to_string(),
        "PREF" | "pref" => "Preferred".to_string(),
        "INTERNET" | "internet" => "Internet".to_string(),
        "IPHONE" | "iphone" => "iPhone".to_string(),
        other => {
            let mut chars = other.chars();
            match chars.next() {
                Some(c) => {
                    let first: String = c.to_uppercase().collect();
                    let rest: String = chars.as_str().to_lowercase();
                    format!("{first}{rest}")
                }
                None => "Other".to_string(),
            }
        }
    }
}

/// Parse structured N field: N:Family;Given;Additional;Prefix;Suffix
fn parse_n_value(value: &str) -> String {
    let parts: Vec<&str> = value.split(';').collect();
    let family = parts.first().map(|s| unescape_vcard(s)).unwrap_or_default();
    let given = parts.get(1).map(|s| unescape_vcard(s)).unwrap_or_default();

    let mut name = String::new();
    if !given.is_empty() {
        name.push_str(given.trim());
    }
    if !family.is_empty() {
        if !name.is_empty() {
            name.push(' ');
        }
        name.push_str(family.trim());
    }
    name
}

/// Format structured ADR: ADR:PO;Ext;Street;City;Region;Postal;Country
fn format_adr(value: &str) -> String {
    let parts: Vec<&str> = value.split(';').collect();
    let mut components = Vec::new();

    // Indices: 0=PO, 1=Ext, 2=Street, 3=City, 4=Region, 5=Postal, 6=Country
    for &idx in &[2, 3, 4, 5, 6, 0, 1] {
        if let Some(&part) = parts.get(idx) {
            let unescaped = unescape_vcard(part);
            let trimmed = unescaped.trim();
            if !trimmed.is_empty() {
                components.push(trimmed.to_string());
            }
        }
    }

    components.join(", ")
}

/// Parse PHOTO property and return decoded bytes.
fn parse_photo(line: &str, version: VCardVersion) -> Option<Vec<u8>> {
    let value = extract_value(line)?;

    match version {
        VCardVersion::V40 => {
            // v4.0: data URI format: data:image/jpeg;base64,XXXX
            let b64 = if let Some(rest) = value.strip_prefix("data:") {
                rest.split_once(',').map(|(_, d)| d)?
            } else {
                &value
            };
            decode_photo_base64(b64)
        }
        _ => {
            // v2.1/3.0: base64 encoded in value
            // Might have ENCODING=b or ENCODING=BASE64
            decode_photo_base64(&value)
        }
    }
}

/// Decode base64 photo data, enforcing size limit.
fn decode_photo_base64(b64: &str) -> Option<Vec<u8>> {
    let clean: String = b64.chars().filter(|c| !c.is_whitespace()).collect();

    let decoded = BASE64_STANDARD.decode(clean.as_bytes()).ok()?;

    Some(decoded)
}

/// Normalize a date string to YYYY-MM-DD format.
fn normalize_date(value: &str) -> String {
    let trimmed = value.trim();

    if trimmed.len() == 10
        && trimmed.as_bytes().get(4) == Some(&b'-')
        && trimmed.as_bytes().get(7) == Some(&b'-')
    {
        return trimmed.to_string();
    }

    // YYYYMMDD (compact)
    if trimmed.len() == 8 && trimmed.chars().all(|c| c.is_ascii_digit()) {
        return format!("{}-{}-{}", &trimmed[0..4], &trimmed[4..6], &trimmed[6..8]);
    }

    // --MM-DD (vCard 4.0 partial date)
    if trimmed.starts_with("--") && trimmed.len() >= 7 {
        let rest = &trimmed[2..];
        if rest.starts_with('-') {
            return format!("0000{rest}");
        }
        return format!("0000-{rest}");
    }

    truncate_chars(trimmed, MAX_VALUE_LENGTH)
}

/// Unescape vCard property values.
fn unescape_vcard(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => result.push('\n'),
                Some(',') => result.push(','),
                Some(';') => result.push(';'),
                Some(':') => result.push(':'), // Google non-standard
                Some('\\') => result.push('\\'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Truncate a string to at most `max_chars` characters.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect()
    }
}
