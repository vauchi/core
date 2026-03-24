// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen building and engine creation for `AppEngine`.

use std::collections::HashMap;

use super::AppEngine;
use super::AppScreen;
use super::initials;
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::component::{ContactItem, FieldDisplay, UiFieldVisibility};
use crate::ui::contact_detail::{ContactDetailEngine, ContactNotFoundEngine, SharedInfoView};
use crate::ui::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use crate::ui::contact_limit::ContactLimitEngine;
use crate::ui::contact_list::ContactListEngine;
use crate::ui::contact_merge::{ContactMergeEngine, MergePreview};
use crate::ui::contact_visibility::ContactVisibilityEngine;
use crate::ui::delivery::DeliveryStatusEngine;
use crate::ui::device_linking::DeviceLinkingEngine;
use crate::ui::duplicate_detection::DuplicateDetectionEngine;
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::emergency_shred::EmergencyShredEngine;
use crate::ui::engine::WorkflowEngine;
use crate::ui::exchange::{ExchangeConfig, ExchangeEngine};
use crate::ui::form_dialog::FormDialogEngine;
use crate::ui::gdpr::GdprEngine;
use crate::ui::group_detail::GroupDetailEngine;
use crate::ui::groups_list::{GroupInfo, GroupsEngine, GroupsMode};
use crate::ui::help::{HelpEngine, HelpItem};
use crate::ui::lock_screen::LockScreenEngine;
use crate::ui::more::MoreEngine;
use crate::ui::my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, OwnFieldInfo};
use crate::ui::my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
use crate::ui::onboarding::OnboardingEngine;
use crate::ui::recovery_status::RecoveryEngine;
use crate::ui::settings::{SettingsConfig, SettingsEngine};
use crate::ui::support::SupportEngine;
use crate::ui::sync_status::SyncStatusEngine;
use crate::ui::tor_settings::TorSettingsEngine;
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_engine(vauchi: &Vauchi, screen: &AppScreen) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(OnboardingEngine::new()),
            AppScreen::MyInfo => {
                let progress = MyInfoProgress::default();
                let all_groups = vauchi.list_groups().unwrap_or_default();

                // Build own card fields with visibility info
                let (display_name, own_fields) = match vauchi.own_card() {
                    Ok(Some(card)) => {
                        let name = card.display_name().to_string();
                        let fields: Vec<OwnFieldInfo> = card
                            .fields()
                            .iter()
                            .map(|f| {
                                // Which groups can see this field?
                                let visible_groups: Vec<String> = all_groups
                                    .iter()
                                    .filter(|g| g.is_field_visible(f.id()))
                                    .map(|g| g.name().to_string())
                                    .collect();
                                // Count contacts across visible groups (deduplicated)
                                let mut visible_contact_ids =
                                    std::collections::HashSet::<String>::new();
                                for g in &all_groups {
                                    if g.is_field_visible(f.id()) {
                                        for cid in g.contacts() {
                                            visible_contact_ids.insert(cid.to_string());
                                        }
                                    }
                                }
                                OwnFieldInfo {
                                    field_id: f.id().to_string(),
                                    field_type: format!("{:?}", f.field_type()),
                                    label: f.label().to_string(),
                                    value: f.value().to_string(),
                                    visible_groups,
                                    contact_count: visible_contact_ids.len(),
                                }
                            })
                            .collect();
                        (name, fields)
                    }
                    _ => (String::new(), Vec::new()),
                };

                // Build group tabs
                let group_tabs: Vec<MyInfoGroupTab> = all_groups
                    .iter()
                    .map(|g| {
                        let field_indices: Vec<usize> = own_fields
                            .iter()
                            .enumerate()
                            .filter(|(_, f)| g.is_field_visible(&f.field_id))
                            .map(|(i, _)| i)
                            .collect();
                        MyInfoGroupTab {
                            group_id: g.id().to_string(),
                            group_name: g.name().to_string(),
                            field_indices,
                        }
                    })
                    .collect();

                Box::new(
                    MyInfoEngine::new(progress)
                        .with_own_card(display_name, own_fields)
                        .with_groups(group_tabs),
                )
            }
            AppScreen::MyInfoEntryDetail { field_id } => {
                Self::create_entry_detail_engine(vauchi, field_id)
            }
            AppScreen::Contacts => {
                let contacts = Self::load_contact_items(vauchi);
                let all_groups = vauchi.list_groups().unwrap_or_default();
                if all_groups.is_empty() {
                    Box::new(ContactListEngine::new(contacts))
                } else {
                    let groups: Vec<(String, String)> = all_groups
                        .iter()
                        .map(|g| (g.id().to_string(), g.name().to_string()))
                        .collect();
                    let mut memberships = HashMap::new();
                    for g in &all_groups {
                        let member_ids: Vec<String> = contacts
                            .iter()
                            .filter(|c| g.contains_contact(&c.id))
                            .map(|c| c.id.clone())
                            .collect();
                        memberships.insert(g.id().to_string(), member_ids);
                    }
                    Box::new(ContactListEngine::with_groups(
                        contacts,
                        groups,
                        memberships,
                    ))
                }
            }
            AppScreen::Settings => {
                let card = vauchi.own_card().ok().flatten();
                let display_name = card
                    .map(|c| c.display_name().to_string())
                    .unwrap_or_default();
                let config = SettingsConfig {
                    display_name,
                    delivery_receipts_enabled: vauchi.config().delivery_receipts_enabled,
                    suppress_presence: vauchi.config().suppress_presence,
                    relay_url: vauchi.config().relay.server_url.clone(),
                    device_count: 1,
                    password_set: vauchi.is_password_enabled().unwrap_or(false),
                };
                Box::new(SettingsEngine::new(config))
            }
            AppScreen::Exchange => {
                let card = vauchi.own_card().ok().flatten();
                let available_groups = vauchi
                    .list_groups()
                    .unwrap_or_default()
                    .iter()
                    .map(|g| (g.id().to_string(), g.name().to_string()))
                    .collect();
                let config = ExchangeConfig {
                    own_name: card
                        .as_ref()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default(),
                    own_qr_data: vauchi.public_id().unwrap_or_default(),
                    available_groups,
                };

                // ADR-031: Create a protocol session if identity + card are available.
                // Identity is cloned via storage serialization (it intentionally
                // doesn't impl Clone because it contains private key material).
                // The intermediate buffer is zeroized to avoid leaking key material.
                let session = vauchi
                    .identity()
                    .and_then(|id_ref| {
                        let bytes = zeroize::Zeroizing::new(id_ref.to_storage_bytes());
                        vauchi_core::identity::Identity::from_storage_bytes(&bytes).ok()
                    })
                    .and_then(|identity| {
                        card.map(|c| {
                            let proximity =
                                vauchi_core::exchange::ManualConfirmationVerifier::new();
                            vauchi_core::exchange::ExchangeSession::new_qr(identity, c, proximity)
                        })
                    });

                match session {
                    Some(s) => Box::new(ExchangeEngine::with_session(config, s)),
                    None => Box::new(ExchangeEngine::new(config)),
                }
            }
            AppScreen::Help => Box::new(HelpEngine::new(Self::default_help_items())),
            AppScreen::Backup => Box::new(BackupRecoveryEngine::new(None)),
            AppScreen::Lock => Box::new(LockScreenEngine::new(5)),
            AppScreen::DeviceLinking => {
                let qr = vauchi.public_id().unwrap_or_default();
                Box::new(DeviceLinkingEngine::new(qr))
            }
            AppScreen::DuressPin => {
                let config = vauchi
                    .load_duress_settings()
                    .ok()
                    .flatten()
                    .map(|s| {
                        let alert_contacts = s
                            .alert_contact_ids
                            .iter()
                            .filter_map(|id| {
                                vauchi.get_contact(id).ok().flatten().map(|c| ContactItem {
                                    id: c.id().to_string(),
                                    name: c.display_name().to_string(),
                                    subtitle: None,
                                    avatar_initials: initials(c.display_name()),
                                    status: None,
                                    searchable_fields: vec![],
                                })
                            })
                            .collect();
                        DuressConfig {
                            enabled: true,
                            alert_contacts,
                            alert_message: s.alert_message.clone(),
                            include_location: s.include_location,
                        }
                    })
                    .unwrap_or_default();
                Box::new(DuressPinEngine::new(config))
            }
            AppScreen::EmergencyShred => Box::new(EmergencyShredEngine::new()),
            AppScreen::DeliveryStatus => Box::new(DeliveryStatusEngine::new(vec![])),
            AppScreen::Sync => {
                let relay_url = vauchi.config().relay.server_url.clone();
                let contact_count = vauchi.list_contacts().map(|c| c.len()).unwrap_or(0);
                Box::new(SyncStatusEngine::new(relay_url, contact_count, 0))
            }
            AppScreen::TorSettings => Box::new(TorSettingsEngine::new(false, false)),
            AppScreen::Recovery => {
                let contacts = Self::load_contact_items(vauchi);
                Box::new(RecoveryEngine::new(contacts, 3))
            }
            AppScreen::Groups => {
                let all_groups = vauchi.list_groups().unwrap_or_default();
                let contacts = Self::load_contact_items(vauchi);
                let group_infos: Vec<GroupInfo> = all_groups
                    .iter()
                    .map(|g| {
                        let member_count = contacts
                            .iter()
                            .filter(|c| g.contains_contact(&c.id))
                            .count();
                        GroupInfo {
                            id: g.id().to_string(),
                            name: g.name().to_string(),
                            member_count,
                            visible_field_count: g.visible_fields().len(),
                        }
                    })
                    .collect();
                Box::new(GroupsEngine::new(group_infos, GroupsMode::Members))
            }
            AppScreen::GroupDetail { group_id } => {
                let group_name = vauchi
                    .get_group(group_id)
                    .ok()
                    .map(|g| g.name().to_string())
                    .unwrap_or_else(|| "Group".into());
                let mut members: Vec<ContactItem> = vauchi
                    .get_group_members(group_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| ContactItem {
                        id: c.id().to_string(),
                        name: c.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(c.display_name()),
                        status: None,
                        searchable_fields: vec![],
                    })
                    .collect();
                members.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
                Box::new(GroupDetailEngine::new(
                    group_id.clone(),
                    group_name,
                    members,
                ))
            }
            AppScreen::Privacy => {
                let contact_count = vauchi.contact_count().unwrap_or(0);
                Box::new(
                    GdprEngine::new(None, "Active".into()).with_deletion_summary(
                        crate::ui::gdpr::DeletionSummary {
                            contact_count,
                            has_backup: false,
                            device_count: 1,
                        },
                    ),
                )
            }
            AppScreen::Support => Box::new(SupportEngine::new()),
            AppScreen::FormDialog { dialog_type } => {
                Box::new(FormDialogEngine::new(dialog_type.clone()))
            }
            AppScreen::More => Box::new(MoreEngine::new()),
            AppScreen::ContactDetail { contact_id } => match vauchi.get_contact(contact_id) {
                Ok(Some(contact)) => {
                    let fields: Vec<FieldDisplay> = contact
                        .card()
                        .fields()
                        .iter()
                        .map(|f| FieldDisplay {
                            id: f.id().to_string(),
                            field_type: format!("{:?}", f.field_type()),
                            label: f.label().to_string(),
                            value: f.value().to_string(),
                            visibility: UiFieldVisibility::Shown,
                        })
                        .collect();
                    let status = if vauchi.is_contact_revoked(contact.id()) {
                        Some("Deleted their identity".into())
                    } else if contact.has_recovered() && !contact.is_fingerprint_verified() {
                        Some("Recovered — re-verify recommended".into())
                    } else {
                        None
                    };
                    let item = ContactItem {
                        id: contact.id().to_string(),
                        name: contact.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(contact.display_name()),
                        status,
                        searchable_fields: contact
                            .card()
                            .fields()
                            .iter()
                            .map(|f| f.value().to_string())
                            .collect(),
                    };

                    // Load personal note (stored as raw UTF-8 bytes by the app layer)
                    let personal_note = vauchi
                        .load_personal_notes(contact_id)
                        .ok()
                        .flatten()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .unwrap_or_default();

                    // Load per-field notes — convert raw bytes to UTF-8 strings
                    let field_notes: HashMap<String, String> = vauchi
                        .load_contact_field_notes(contact_id)
                        .unwrap_or_default()
                        .into_iter()
                        .filter_map(|(field_id, bytes)| {
                            String::from_utf8(bytes).ok().map(|s| (field_id, s))
                        })
                        .collect();

                    // Build shared info (my card as seen by this contact)
                    let shared_info = Self::build_shared_info(vauchi, contact_id);

                    match shared_info {
                        Some(info) => Box::new(
                            ContactDetailEngine::with_shared_info(
                                item,
                                fields,
                                info,
                                personal_note,
                            )
                            .with_field_notes(field_notes),
                        ),
                        None => Box::new(
                            ContactDetailEngine::new(item, fields, personal_note)
                                .with_field_notes(field_notes),
                        ),
                    }
                }
                _ => Box::new(ContactNotFoundEngine::new(contact_id.clone())),
            },
            AppScreen::ContactVisibility { contact_id } => {
                let (name, fields) = match vauchi.get_contact(contact_id) {
                    Ok(Some(contact)) => {
                        let name = contact.display_name().to_string();
                        let items = contact
                            .card()
                            .fields()
                            .iter()
                            .map(|f| crate::ui::component::ToggleItem {
                                id: f.id().to_string(),
                                label: f.label().to_string(),
                                selected: true,
                                subtitle: None,
                            })
                            .collect();
                        (name, items)
                    }
                    _ => (
                        format!("Contact {}", &contact_id[..8.min(contact_id.len())]),
                        vec![],
                    ),
                };
                Box::new(ContactVisibilityEngine::new(
                    contact_id.clone(),
                    name,
                    fields,
                ))
            }
            AppScreen::ContactEdit { contact_id } => match vauchi.get_contact(contact_id) {
                Ok(Some(contact)) => {
                    let fields = contact
                        .card()
                        .fields()
                        .iter()
                        .map(|f| EditableField {
                            id: f.id().to_string(),
                            field_type: format!("{:?}", f.field_type()),
                            label: f.label().to_string(),
                            value: f.value().to_string(),
                            visible_to_groups: vec![],
                            shown: true,
                        })
                        .collect();
                    let editable = EditableContact {
                        display_name: contact.display_name().to_string(),
                        fields,
                    };
                    Box::new(ContactEditEngine::new(editable, vec![]))
                }
                _ => Box::new(ContactNotFoundEngine::new(contact_id.clone())),
            },
            AppScreen::ContactDuplicates => Box::new(DuplicateDetectionEngine::new(vec![])),
            AppScreen::ContactMerge {
                primary_name,
                primary_fields,
                secondary_name,
                secondary_fields,
            } => Box::new(ContactMergeEngine::new(MergePreview {
                primary_name: primary_name.clone(),
                primary_fields: primary_fields.clone(),
                secondary_name: secondary_name.clone(),
                secondary_fields: secondary_fields.clone(),
            })),
            AppScreen::ContactLimit => {
                let contact_count = vauchi.list_contacts().map(|c| c.len()).unwrap_or(0);
                Box::new(ContactLimitEngine::new(contact_count, 0))
            }
        }
    }

    /// Builds a SharedInfoView for a contact — my fields as visible to them.
    fn build_shared_info(vauchi: &Vauchi, contact_id: &str) -> Option<SharedInfoView> {
        let own_card = vauchi.own_card().ok()??;

        // Determine the display name this contact sees
        // Check groups the contact is in for a display_name_override
        let groups = vauchi
            .get_groups_for_contact(contact_id)
            .unwrap_or_default();
        let shared_display_name = groups
            .iter()
            .find_map(|g| g.display_name_override().map(|s| s.to_string()))
            .unwrap_or_else(|| own_card.display_name().to_string());

        // Build my fields with effective visibility for this contact
        let my_fields: Vec<FieldDisplay> = own_card
            .fields()
            .iter()
            .map(|f| {
                let is_visible = vauchi
                    .get_effective_field_visibility(contact_id, f.id())
                    .unwrap_or(true);
                FieldDisplay {
                    id: f.id().to_string(),
                    field_type: format!("{:?}", f.field_type()),
                    label: f.label().to_string(),
                    value: f.value().to_string(),
                    visibility: if is_visible {
                        UiFieldVisibility::Shown
                    } else {
                        UiFieldVisibility::Hidden
                    },
                }
            })
            .collect();

        let visible_groups: Vec<String> = groups.iter().map(|g| g.name().to_string()).collect();

        Some(SharedInfoView {
            shared_display_name,
            my_fields,
            visible_groups,
        })
    }

    fn create_entry_detail_engine(vauchi: &Vauchi, field_id: &str) -> Box<dyn WorkflowEngine> {
        let card = vauchi.own_card().ok().flatten();
        let all_groups = vauchi.list_groups().unwrap_or_default();

        let field = card
            .as_ref()
            .and_then(|c| c.fields().iter().find(|f| f.id() == field_id).cloned());

        let Some(field) = field else {
            // Field not found — return a minimal engine
            return Box::new(MyInfoEntryDetailEngine::new(
                field_id.to_string(),
                "Unknown".into(),
                "Unknown".into(),
                "Field not found".into(),
                None,
                vec![],
                vec![],
            ));
        };

        // Build group visibility state
        let groups: Vec<(String, String, bool)> = all_groups
            .iter()
            .map(|g| {
                (
                    g.id().to_string(),
                    g.name().to_string(),
                    g.is_field_visible(field_id),
                )
            })
            .collect();

        // Build contact list from groups that can see this field
        let mut visible_contacts = Vec::new();
        let mut seen_contacts = std::collections::HashSet::new();
        for g in &all_groups {
            if g.is_field_visible(field_id) {
                for cid in g.contacts() {
                    if seen_contacts.insert(cid.to_string()) {
                        let name = vauchi
                            .get_contact(cid)
                            .ok()
                            .flatten()
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| "Unknown".into());
                        visible_contacts.push(EntryContactInfo {
                            contact_id: cid.to_string(),
                            name,
                            via_group: g.name().to_string(),
                        });
                    }
                }
            }
        }

        Box::new(MyInfoEntryDetailEngine::new(
            field_id.to_string(),
            format!("{:?}", field.field_type()),
            field.label().to_string(),
            field.value().to_string(),
            field.note().map(|s| s.to_string()),
            groups,
            visible_contacts,
        ))
    }

    pub(super) fn load_contact_items(vauchi: &Vauchi) -> Vec<ContactItem> {
        match vauchi.list_contacts() {
            Ok(contacts) => contacts
                .iter()
                .map(|c| {
                    let fields: Vec<String> = c
                        .card()
                        .fields()
                        .iter()
                        .map(|f| f.value().to_string())
                        .collect();
                    let subtitle = fields.first().cloned();
                    let status = if vauchi.is_contact_revoked(c.id()) {
                        Some("Deleted their identity".into())
                    } else if c.has_recovered() && !c.is_fingerprint_verified() {
                        Some("Recovered — re-verify recommended".into())
                    } else {
                        None
                    };
                    ContactItem {
                        id: c.id().to_string(),
                        name: c.display_name().to_string(),
                        subtitle,
                        avatar_initials: initials(c.display_name()),
                        status,
                        searchable_fields: fields,
                    }
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    fn default_help_items() -> Vec<HelpItem> {
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
                answer_url: Some("https://docs.vauchi.app/faq/add-contact".into()),
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
                answer_url: Some("https://docs.vauchi.app/faq/e2e".into()),
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
                answer_url: Some("https://docs.vauchi.app/faq/backup".into()),
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
                answer_url: Some("https://docs.vauchi.app/faq/recovery".into()),
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
                answer_url: Some("https://docs.vauchi.app/faq/exchange".into()),
                category: "Getting Started".into(),
            },
            HelpItem {
                id: "tor-privacy".into(),
                question: "How does Tor routing protect my privacy?".into(),
                answer: Some(
                    "When enabled, Vauchi routes relay connections through Tor, \
                     hiding your IP address from the relay server. This prevents \
                     the server from learning your location or network identity. \
                     Enable it in Settings > Tor Privacy."
                        .into(),
                ),
                answer_url: Some("https://docs.vauchi.app/faq/tor".into()),
                category: "Privacy".into(),
            },
        ]
    }
}
