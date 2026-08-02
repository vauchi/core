// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Static help/support catalog rendered by `HelpEngine` — FAQ items
//! and the bug-report / idea mailto builders.

use crate::ui::help::HelpItem;

pub(super) fn default_help_items() -> Vec<HelpItem> {
    vec![
        HelpItem {
            id: "add-contact".into(),
            question: "How do I add a contact?".into(),
            answer: Some(
                "Meet in person and go to Exchange. \
                 Show your QR code or use Bluetooth to share your contact card. \
                 Both parties must be present — Vauchi never exchanges contacts remotely."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#contacts--exchange".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "e2e-encryption".into(),
            question: "What is end-to-end encryption?".into(),
            answer: Some(
                "End-to-end encryption means only you and your contact can read \
                 your shared data. The relay server sees only encrypted blobs — \
                 it cannot read names, fields, or any content. Keys are exchanged \
                 in person and never leave your device."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#privacy--security".into()),
            category: "Security".into(),
        },
        HelpItem {
            id: "create-backup".into(),
            question: "How do I create a backup?".into(),
            answer: Some(
                "Go to Settings > Backup & Restore. Choose Export to create an \
                 encrypted backup file. Store it safely — you will need your \
                 password to restore it. Backups include your identity, contacts, \
                 and all field data."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#backup--restore".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "recovery".into(),
            question: "How does social recovery work?".into(),
            answer: Some(
                "Social recovery lets trusted contacts help you regain access \
                 if you lose your device. You choose recovery trustees from your \
                 contacts. To recover, a threshold of trustees must confirm your \
                 identity in person."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#identity--account".into()),
            category: "Security".into(),
        },
        HelpItem {
            id: "exchange-qr".into(),
            question: "How do I exchange contact cards?".into(),
            answer: Some(
                "Go to Exchange to show your QR code. Your contact scans it \
                 with their Vauchi app (or vice versa). This establishes an \
                 encrypted channel so future updates sync automatically. \
                 Both parties must be physically present."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#contacts--exchange".into()),
            category: "Getting Started".into(),
        },
        HelpItem {
            id: "ip-privacy".into(),
            question: "How is my IP address protected?".into(),
            answer: Some(
                "Vauchi uses a self-hosted OHTTP relay that strips your IP \
                 address before requests reach the relay server. For additional \
                 protection you can configure a SOCKS5 proxy in Settings. \
                 Timing obfuscation further prevents traffic correlation."
                    .into(),
            ),
            answer_url: Some("https://vauchi.app/docs/users/faq#privacy--security".into()),
            category: "Privacy".into(),
        },
        HelpItem {
            id: "report-issue".into(),
            question: "Report a Bug".into(),
            answer: None,
            answer_url: Some(bug_report_mailto()),
            category: "Support".into(),
        },
        HelpItem {
            id: "feature-idea".into(),
            question: "Suggest an Idea".into(),
            answer: None,
            answer_url: Some(idea_mailto()),
            category: "Support".into(),
        },
        HelpItem {
            id: "known-issues".into(),
            question: "Known Issues".into(),
            answer: None,
            answer_url: Some("https://vauchi.app/docs/users/known-issues".into()),
            category: "Support".into(),
        },
    ]
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
