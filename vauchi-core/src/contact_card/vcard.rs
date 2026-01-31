// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! vCard 4.0 Export/Import (RFC 6350)

use crate::contact_card::{ContactCard, ContactField, FieldType};

/// Exports a ContactCard to vCard 4.0 format.
pub fn export_vcard(card: &ContactCard) -> String {
    let mut lines = Vec::new();
    lines.push("BEGIN:VCARD".to_string());
    lines.push("VERSION:4.0".to_string());
    lines.push(format!("FN:{}", escape_vcard(card.display_name())));

    for field in card.fields() {
        match field.field_type() {
            FieldType::Phone => {
                lines.push(format!(
                    "TEL;TYPE={}:{}",
                    escape_vcard(field.label()),
                    escape_vcard(field.value())
                ));
            }
            FieldType::Email => {
                lines.push(format!(
                    "EMAIL;TYPE={}:{}",
                    escape_vcard(field.label()),
                    escape_vcard(field.value())
                ));
            }
            FieldType::Website => {
                lines.push(format!("URL:{}", escape_vcard(field.value())));
            }
            FieldType::Address => {
                lines.push(format!(
                    "ADR;TYPE={}:;;{};;;;",
                    escape_vcard(field.label()),
                    escape_vcard(field.value())
                ));
            }
            FieldType::Social => {
                lines.push(format!(
                    "X-SOCIALPROFILE;TYPE={}:{}",
                    escape_vcard(field.label()),
                    escape_vcard(field.value())
                ));
            }
            FieldType::Custom => {
                lines.push(format!(
                    "NOTE;TYPE={}:{}",
                    escape_vcard(field.label()),
                    escape_vcard(field.value())
                ));
            }
        }
    }

    lines.push("END:VCARD".to_string());
    lines.join("\r\n")
}

/// Imports a vCard string into a ContactCard.
pub fn import_vcard(vcard: &str) -> Result<ContactCard, VCardError> {
    let lines: Vec<&str> = vcard.lines().collect();

    if lines.is_empty() || !lines[0].trim().eq_ignore_ascii_case("BEGIN:VCARD") {
        return Err(VCardError::InvalidFormat("Missing BEGIN:VCARD".into()));
    }

    let mut display_name = String::new();
    let mut fields = Vec::new();

    for line in &lines {
        let line = line.trim();
        if let Some(value) = line.strip_prefix("FN:") {
            display_name = unescape_vcard(value);
        } else if line.starts_with("TEL") {
            let (label, value) = parse_typed_field(line, "TEL");
            fields.push((FieldType::Phone, label, value));
        } else if line.starts_with("EMAIL") {
            let (label, value) = parse_typed_field(line, "EMAIL");
            fields.push((FieldType::Email, label, value));
        } else if let Some(value) = line.strip_prefix("URL:") {
            fields.push((FieldType::Website, "Website".to_string(), unescape_vcard(value)));
        } else if line.starts_with("ADR") {
            let (label, value) = parse_typed_field(line, "ADR");
            // ADR format: ;;street;;;;
            let addr = value.replace(";;", "").replace(";;;;", "").trim().to_string();
            if !addr.is_empty() {
                fields.push((FieldType::Address, label, addr));
            }
        }
    }

    if display_name.is_empty() {
        return Err(VCardError::MissingField("FN (display name)".into()));
    }

    let mut card = ContactCard::new(&display_name);
    for (field_type, label, value) in fields {
        let _ = card.add_field(ContactField::new(field_type, &label, &value));
    }

    Ok(card)
}

fn parse_typed_field(line: &str, prefix: &str) -> (String, String) {
    // Format: PREFIX;TYPE=label:value or PREFIX:value
    let after_prefix = &line[prefix.len()..];
    if let Some(colon_pos) = after_prefix.find(':') {
        let params = &after_prefix[..colon_pos];
        let value = unescape_vcard(&after_prefix[colon_pos + 1..]);
        let label = params
            .split(';')
            .find_map(|p| p.strip_prefix("TYPE="))
            .unwrap_or("Other")
            .to_string();
        (label, value)
    } else {
        ("Other".to_string(), unescape_vcard(after_prefix))
    }
}

fn escape_vcard(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

fn unescape_vcard(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}

/// vCard parsing errors.
#[derive(Debug, thiserror::Error)]
pub enum VCardError {
    #[error("Invalid vCard format: {0}")]
    InvalidFormat(String),
    #[error("Missing required field: {0}")]
    MissingField(String),
}
