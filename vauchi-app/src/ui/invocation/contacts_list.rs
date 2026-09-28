// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: GPL-3.0-or-later

use serde::Serialize;
use vauchi_core::{
    BindingId, Command, Contact, ContactCard, DocumentSpec, PresentationNode, PresentationRow,
    PresentationTextStyle, VauchiResult, api::Vauchi,
};

use super::{InvocationOutput, accessibility, one_shot_surface};
use crate::i18n::{Locale, get_string, get_string_with_args};

const SCHEMA: &str = "vauchi.contacts.v1";
const SURFACE_ID: &str = "invocation.contacts_list";
const ARCHIVED_SURFACE_ID: &str = "invocation.contacts_list_archived";
const SHORT_ID_LEN: usize = 8;

/// `vauchi.contacts.v1` — the shape the CLI's `--raw` printed before this
/// moved into Core; e2e parses it, so fields are only ever added.
#[derive(Serialize)]
struct ContactDocument {
    id: String,
    display_name: String,
    fingerprint_verified: bool,
    recovery_trusted: bool,
    card: CardDocument,
}

#[derive(Serialize)]
struct CardDocument {
    display_name: String,
    fields: Vec<FieldDocument>,
}

#[derive(Serialize)]
struct FieldDocument {
    field_type: String,
    label: String,
    value: String,
}

impl From<&Contact> for ContactDocument {
    fn from(contact: &Contact) -> Self {
        Self {
            id: contact.id().to_string(),
            display_name: contact.display_name().to_string(),
            fingerprint_verified: contact.is_fingerprint_verified(),
            recovery_trusted: contact.is_recovery_trusted(),
            card: CardDocument::from(contact.card()),
        }
    }
}

impl From<&ContactCard> for CardDocument {
    fn from(card: &ContactCard) -> Self {
        Self {
            display_name: card.display_name().to_string(),
            fields: card
                .fields()
                .iter()
                .map(|field| FieldDocument {
                    // v1 froze the Debug spelling of the field type.
                    field_type: format!("{:?}", field.field_type()),
                    label: field.label().to_string(),
                    value: field.value().to_string(),
                })
                .collect(),
        }
    }
}

pub(super) fn run(
    vauchi: &Vauchi,
    offset: usize,
    limit: usize,
    output: InvocationOutput,
    locale: Locale,
) -> VauchiResult<Vec<Command>> {
    let paginated = offset > 0 || limit > 0;
    let contacts = if paginated {
        vauchi.list_contacts_paginated(offset, limit)?
    } else {
        vauchi.list_contacts()?
    };

    Ok(vec![match output {
        InvocationOutput::Document => document(&contacts),
        InvocationOutput::Text => {
            let total = vauchi.contact_count()?;
            active_surface(&contacts, total, paginated.then_some(offset), locale)
        }
    }])
}

pub(super) fn run_archived(
    vauchi: &Vauchi,
    output: InvocationOutput,
    locale: Locale,
) -> VauchiResult<Vec<Command>> {
    let archived = vauchi.list_archived_contacts()?;
    Ok(vec![match output {
        InvocationOutput::Document => document(&archived),
        InvocationOutput::Text => {
            let title = get_string_with_args(
                locale,
                "cli.contacts.archived.header",
                &[("count", &archived.len().to_string())],
            );
            if archived.is_empty() {
                let empty = get_string(locale, "archived_contacts.empty");
                one_shot_surface(
                    ARCHIVED_SURFACE_ID,
                    title,
                    vec![text(empty, PresentationTextStyle::Body)],
                )
            } else {
                list_surface(ARCHIVED_SURFACE_ID, title, rows(&archived, locale))
            }
        }
    }])
}

fn document(contacts: &[Contact]) -> Command {
    let entries: Vec<ContactDocument> = contacts.iter().map(ContactDocument::from).collect();
    Command::EmitDocument {
        document: DocumentSpec {
            media_type: "application/json".into(),
            schema: SCHEMA.into(),
            data: serde_json::to_vec_pretty(&entries)
                .expect("contact documents contain only strings and booleans"),
        },
    }
}

fn active_surface(
    contacts: &[Contact],
    total: usize,
    page_offset: Option<usize>,
    locale: Locale,
) -> Command {
    if total == 0 {
        let hint = get_string(locale, "cli.contacts.list.no_contacts");
        let command = get_string(locale, "cli.contacts.list.exchange_command");
        let title = get_string_with_args(locale, "cli.contacts.list.header", &[("count", "0")]);
        return one_shot_surface(
            SURFACE_ID,
            title,
            vec![
                text(hint, PresentationTextStyle::Body),
                text(command, PresentationTextStyle::Monospace),
            ],
        );
    }

    let title = match page_offset {
        Some(offset) => get_string_with_args(
            locale,
            "cli.contacts.list.paginated_header",
            &[
                ("start", &(offset + 1).to_string()),
                ("end", &(offset + contacts.len()).to_string()),
                ("total", &total.to_string()),
            ],
        ),
        None => get_string_with_args(
            locale,
            "cli.contacts.list.header",
            &[("count", &total.to_string())],
        ),
    };
    list_surface(SURFACE_ID, title, rows(contacts, locale))
}

fn rows(contacts: &[Contact], locale: Locale) -> Vec<PresentationRow> {
    let verified = get_string(locale, "contacts.verified");
    let not_verified = get_string(locale, "contacts.not_verified");
    let recovery_trusted = get_string(locale, "contact_detail.recovery_trusted_label");

    contacts
        .iter()
        .map(|contact| {
            let verification = if contact.is_fingerprint_verified() {
                &verified
            } else {
                &not_verified
            };
            let status = if contact.is_recovery_trusted() {
                format!("{verification} · {recovery_trusted}")
            } else {
                verification.clone()
            };
            PresentationRow {
                title: contact.display_name().to_string(),
                subtitle: Some(status.clone()),
                detail: Some(contact.id().chars().take(SHORT_ID_LEN).collect()),
                icon_token: None,
                image_data: None,
                fallback_text: None,
                selected: false,
                enabled: true,
                activation: None,
                secondary_actions: Vec::new(),
                controls: Vec::new(),
                accessibility: accessibility(&format!("{}, {status}", contact.display_name())),
            }
        })
        .collect()
}

fn list_surface(surface_id: &str, title: String, rows: Vec<PresentationRow>) -> Command {
    one_shot_surface(
        surface_id,
        title.clone(),
        vec![PresentationNode::List {
            id: BindingId::new("contacts").expect("static binding id is valid"),
            label: None,
            rows,
            searchable: false,
            paging: None,
            accessibility: accessibility(&title),
            style: vauchi_core::PresentationListStyle::Rows,
        }],
    )
}

fn text(content: String, style: PresentationTextStyle) -> PresentationNode {
    PresentationNode::Text {
        id: None,
        accessibility: accessibility(&content),
        content,
        style,
    }
}
