// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Static help/support catalog rendered by `HelpEngine` — the FAQ
//! highlights, the remaining questions, and the MORE rows (support
//! mail drafts, privacy policy, about).

use crate::i18n::{Locale, get_string, get_string_with_args};
use crate::ui::help::{HelpItem, SECTION_FAQ, SECTION_HIGHLIGHTS, SECTION_MORE};

const FAQ_URL: &str = "https://vauchi.app/docs/users/faq";

fn highlight(
    locale: Locale,
    id: &str,
    question_key: &str,
    answer_key: &str,
    anchor: &str,
) -> HelpItem {
    HelpItem {
        id: id.into(),
        question: get_string(locale, question_key),
        subtitle: None,
        answer: Some(get_string(locale, answer_key)),
        answer_url: Some(format!("{FAQ_URL}#{anchor}")),
        category: SECTION_HIGHLIGHTS.into(),
    }
}

fn faq(id: &str, question: &str, answer: &str, anchor: &str) -> HelpItem {
    HelpItem {
        id: id.into(),
        question: question.into(),
        subtitle: None,
        answer: Some(answer.into()),
        answer_url: Some(format!("{FAQ_URL}#{anchor}")),
        category: SECTION_FAQ.into(),
    }
}

fn more(id: &str, question: String, subtitle: Option<String>, url: String) -> HelpItem {
    HelpItem {
        id: id.into(),
        question,
        subtitle,
        answer: None,
        answer_url: Some(url),
        category: SECTION_MORE.into(),
    }
}

/// The FAQ highlights are the four questions the design canvas puts
/// first; their copy lives in the locale catalogue
/// (drawn from docs/src/users/faq.md and the tag/group vocabulary).
fn highlights(locale: Locale) -> Vec<HelpItem> {
    vec![
        highlight(
            locale,
            "exchange-flow",
            "faq.exchange_flow.question",
            "faq.exchange_flow.answer",
            "contacts--exchange",
        ),
        highlight(
            locale,
            "relay-sees",
            "faq.relay_sees.question",
            "faq.relay_sees.answer",
            "privacy--security",
        ),
        highlight(
            locale,
            "tag-vs-group",
            "faq.tag_vs_group.question",
            "faq.tag_vs_group.answer",
            "visibility--sharing",
        ),
        highlight(
            locale,
            "lost-phone",
            "faq.lost_phone.question",
            "faq.lost_phone.answer",
            "identity--account",
        ),
    ]
}

fn more_questions() -> Vec<HelpItem> {
    vec![
        faq(
            "add-contact",
            "How do I add a contact?",
            "Meet in person and go to Exchange. \
             Show your QR code or use Bluetooth to share your contact card. \
             Both parties must be present — Vauchi never exchanges contacts remotely.",
            "contacts--exchange",
        ),
        faq(
            "e2e-encryption",
            "What is end-to-end encryption?",
            "End-to-end encryption means only you and your contact can read \
             your shared data. The relay server sees only encrypted blobs — \
             it cannot read names, fields, or any content. Keys are exchanged \
             in person and never leave your device.",
            "privacy--security",
        ),
        faq(
            "create-backup",
            "How do I create a backup?",
            "Go to Settings > Backup. Choose Export to create an \
             encrypted backup file. Store it safely — you will need your \
             password to restore it. Backups include your identity, contacts, \
             and all field data.",
            "backup--restore",
        ),
        faq(
            "recovery",
            "How does social recovery work?",
            "Social recovery lets trusted contacts help you regain access \
             if you lose your device. You choose recovery trustees from your \
             contacts. To recover, a threshold of trustees must confirm your \
             identity in person.",
            "identity--account",
        ),
        faq(
            "exchange-qr",
            "How do I exchange contact cards?",
            "Go to Exchange to show your QR code. Your contact scans it \
             with their Vauchi app (or vice versa). This establishes an \
             encrypted channel so future updates sync automatically. \
             Both parties must be physically present.",
            "contacts--exchange",
        ),
        faq(
            "ip-privacy",
            "How is my IP address protected?",
            "Vauchi uses a self-hosted OHTTP relay that strips your IP \
             address before requests reach the relay server. For additional \
             protection you can configure a SOCKS5 proxy in Settings. \
             Timing obfuscation further prevents traffic correlation.",
            "privacy--security",
        ),
    ]
}

/// The MORE section. About answers inline (what Vauchi is, plus the
/// versions) so it is reachable offline; the other rows leave the app.
fn more_rows(locale: Locale) -> Vec<HelpItem> {
    let versions = about_versions(locale);
    vec![
        more(
            "contact-support",
            get_string(locale, "help.contact"),
            Some(get_string(locale, "help.contact_support_hint")),
            bug_report_mailto(),
        ),
        more(
            "privacy-policy",
            get_string(locale, "help.privacy_policy"),
            None,
            "https://vauchi.app/docs/legal/privacy-policy".into(),
        ),
        HelpItem {
            id: "about".into(),
            question: get_string(locale, "settings.about"),
            subtitle: Some(versions.clone()),
            answer: Some(format!(
                "{}\n\n{versions}",
                get_string(locale, "about.what_is_vauchi.body")
            )),
            answer_url: None,
            category: SECTION_MORE.into(),
        },
        more(
            "feature-idea",
            get_string(locale, "help.suggest_idea"),
            None,
            idea_mailto(),
        ),
        more(
            "known-issues",
            get_string(locale, "help.known_issues"),
            None,
            "https://vauchi.app/docs/users/known-issues".into(),
        ),
    ]
}

/// "Vauchi <binding semver> · core <core semver>". Shells do not pass
/// their own app version through Core yet, so the binding semver stands
/// in for the app (it matches the AAR/XCFramework pin).
fn about_versions(locale: Locale) -> String {
    get_string_with_args(
        locale,
        "help.about_versions",
        &[
            ("app", env!("CARGO_PKG_VERSION")),
            ("core", vauchi_core::VERSION),
        ],
    )
}

pub(super) fn default_help_items(locale: Locale) -> Vec<HelpItem> {
    let mut items = highlights(locale);
    items.extend(more_questions());
    items.extend(more_rows(locale));
    items
}

fn bug_report_mailto() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let subject = percent_encode(&format!("Bug Report — Vauchi v{version}"));
    let body = percent_encode(&format!(
        "--- Device Info (auto-filled) ---\n\
         App: Vauchi v{version}\n\
         Platform: {os} ({arch})\n\
         ---\n\n\
         What happened:\n\n\n\
         Steps to reproduce:\n\
         1. \n\
         2. \n\
         3. \n\n\
         What I expected:\n\n"
    ));
    format!("mailto:support@vauchi.app?subject={subject}&body={body}")
}

fn idea_mailto() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let subject = percent_encode(&format!("Idea — Vauchi v{version}"));
    let body = percent_encode(
        "What would you like to see in Vauchi?\n\n\n\
         Why would this be useful?\n\n",
    );
    format!("mailto:support@vauchi.app?subject={subject}&body={body}")
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from(b"0123456789ABCDEF"[(b >> 4) as usize]));
                out.push(char::from(b"0123456789ABCDEF"[(b & 0x0F) as usize]));
            }
        }
    }
    out
}
