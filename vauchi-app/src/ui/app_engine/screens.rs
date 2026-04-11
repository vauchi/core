// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen building and engine creation for `AppEngine`.

use std::collections::HashMap;

use super::AppEngine;
use super::AppScreen;
use super::initials;
use crate::ui::activity_log::{ActivityLogEngine, ActivityLogItem};
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::component::{A11y, ContactItem, FieldDisplay, Status, UiFieldVisibility};
use crate::ui::contact_detail::{
    ContactDetailEngine, ContactNotFoundEngine, DeliverySummary, SharedInfoView,
};
use crate::ui::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use crate::ui::contact_limit::ContactLimitEngine;
use crate::ui::contact_list::ContactListEngine;
use crate::ui::contact_merge::{ContactMergeEngine, MergePreview};
use crate::ui::contact_visibility::ContactVisibilityEngine;
use crate::ui::delivery::{DeliveryItem, DeliveryStatusEngine};
use crate::ui::device_linking::DeviceLinkingEngine;
use crate::ui::device_management::{DeviceListItem, DeviceManagementEngine};
use crate::ui::duplicate_detection::DuplicateDetectionEngine;
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::emergency_shred::EmergencyShredEngine;
use crate::ui::engine::WorkflowEngine;
use crate::ui::exchange::{ExchangeConfig, ExchangeEngine};
use crate::ui::fingerprint_verify::FingerprintVerifyEngine;
use crate::ui::form_dialog::FormDialogEngine;
use crate::ui::gdpr::GdprEngine;
use crate::ui::group_detail::GroupDetailEngine;
use crate::ui::groups_list::{GroupInfo, GroupsEngine, GroupsMode};
use crate::ui::help::{HelpEngine, HelpItem};
use crate::ui::lock_screen::{DEFAULT_LOCK_MAX_ATTEMPTS, LockScreenEngine};
use crate::ui::more::MoreEngine;
use crate::ui::my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, OwnFieldInfo};
use crate::ui::my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
use crate::ui::onboarding::OnboardingEngine;
use crate::ui::recovery_status::RecoveryEngine;
use crate::ui::settings::{SettingsConfig, SettingsEngine};
use crate::ui::support::SupportEngine;
use crate::ui::sync_status::SyncStatusEngine;
use vauchi_core::api::Vauchi;

impl AppEngine {
    pub(super) fn create_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        preview_as: Option<&str>,
        device_capabilities: &vauchi_core::exchange::capability::types::DeviceCapabilities,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(OnboardingEngine::new()),
            AppScreen::MyInfo => {
                // If a preview-as contact is active, build in PreviewAs view mode.
                if let Some(contact_id) = preview_as {
                    let contact_name = vauchi
                        .get_contact(contact_id)
                        .ok()
                        .flatten()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_else(|| contact_id.to_string());
                    let shared_info = Self::build_shared_info(vauchi, contact_id);
                    let mut engine = MyInfoEngine::new(MyInfoProgress::default()).with_view_mode(
                        crate::ui::my_info::MyInfoViewMode::PreviewAs { contact_name },
                    );
                    if let Some(info) = shared_info {
                        engine = engine.with_preview(info);
                    }
                    return Box::new(engine);
                }

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

                let has_contacts = vauchi.contact_count().unwrap_or(0) > 0;
                Box::new(
                    MyInfoEngine::new(progress)
                        .with_own_card(display_name, own_fields)
                        .with_groups(group_tabs)
                        .with_exchange_prompt(!has_contacts),
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
                    contact_added_notifications: vauchi.config().contact_added_notifications,
                    relay_url: vauchi.config().relay.server_url.clone(),
                    device_count: 1,
                    password_set: vauchi.is_password_enabled().unwrap_or(false),
                    theme: String::new(),
                    available_themes: vec![],
                    language: String::new(),
                    available_languages: vec![],
                    reduce_motion: false,
                    high_contrast: false,
                    large_touch: false,
                    version: String::new(),
                    build: String::new(),
                    sync_status: String::new(),
                    pending_updates: 0,
                    failed_deliveries: 0,
                    debug_mode: false,
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
                let card_snapshot = card
                    .as_ref()
                    .cloned()
                    .map(vauchi_core::exchange::card_snapshot::CardSnapshot::freeze);
                let config = ExchangeConfig {
                    own_name: card
                        .as_ref()
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default(),
                    own_qr_data: vauchi.public_id().unwrap_or_default(),
                    available_groups,
                    device_capabilities: device_capabilities.clone(),
                    mode: None, // triggers mode selection screen
                    card_snapshot,
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
            AppScreen::Backup => Box::new(BackupRecoveryEngine::new(None, vauchi.has_identity())),
            AppScreen::Lock => Box::new(LockScreenEngine::new(DEFAULT_LOCK_MAX_ATTEMPTS)),
            AppScreen::DeviceLinking => {
                let qr_data = vauchi
                    .generate_device_link()
                    .map(|r| r.data_string)
                    .unwrap_or_default();
                Box::new(DeviceLinkingEngine::new(qr_data))
            }
            AppScreen::DeviceManagement => {
                let devices = vauchi
                    .list_devices()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|d| DeviceListItem {
                        device_index: d.device_index,
                        device_name: d.device_name,
                        public_key_prefix: d.public_key_prefix,
                        is_current: d.is_current,
                        is_active: d.is_active,
                    })
                    .collect();
                Box::new(DeviceManagementEngine::new(devices))
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
                                    a11y: Some(A11y {
                                        label: Some(format!("Contact: {}", c.display_name())),
                                        hint: Some("Double tap to view contact details".into()),
                                    }),
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
            AppScreen::DeliveryStatus => {
                let items = Self::load_delivery_items(vauchi);
                Box::new(DeliveryStatusEngine::new(items))
            }
            AppScreen::Sync => {
                let relay_url = vauchi.config().relay.server_url.clone();
                let contact_count = vauchi.list_contacts().map(|c| c.len()).unwrap_or(0);
                let pending = vauchi.pending_update_count().unwrap_or(0) as usize;
                Box::new(SyncStatusEngine::new(relay_url, contact_count, pending))
            }
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
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", c.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                        }),
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
            AppScreen::ActivityLog => {
                use crate::notification_types::ActivityLogEntry;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let rows = vauchi
                    .storage()
                    .activity_log_query_recent(now, 7 * 86400)
                    .unwrap_or_default();
                let contacts: std::collections::HashMap<String, String> = vauchi
                    .list_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (c.id().to_string(), c.display_name().to_string()))
                    .collect();
                let items: Vec<ActivityLogItem> = rows
                    .into_iter()
                    .filter_map(|row| {
                        let entry: ActivityLogEntry = serde_json::from_str(&row.payload).ok()?;
                        let contact_name = contacts
                            .get(entry.contact_id())
                            .cloned()
                            .unwrap_or_else(|| entry.contact_id().to_string());
                        Some(ActivityLogItem {
                            event_key: row.event_key,
                            contact_name,
                            created_at: row.created_at,
                            entry,
                        })
                    })
                    .collect();
                Box::new(ActivityLogEngine::new(items))
            }
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
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", contact.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                        }),
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

                    // Trust data
                    let trust_level = contact.trust_level().to_string();
                    let proposal_trusted = contact.is_proposal_trusted();
                    let is_hidden = contact.is_hidden();
                    let is_imported = contact.is_imported();

                    // Reciprocity status (design spec §6.3)
                    use vauchi_core::exchange::reciprocity::Reciprocity;
                    let reciprocity_status = match contact.reciprocity() {
                        Reciprocity::Pending => "Awaiting confirmation".to_string(),
                        Reciprocity::Unreciprocated => "May not have your card".to_string(),
                        _ => String::new(),
                    };

                    // Delivery status summary (J1: update propagation)
                    let delivery_summary = vauchi
                        .get_delivery_status_for_contact(contact_id)
                        .ok()
                        .map(|records| {
                            use vauchi_core::storage::DeliveryStatus;
                            let total = records.len();
                            let delivered = records
                                .iter()
                                .filter(|r| matches!(r.status, DeliveryStatus::Delivered))
                                .count();
                            let failed = records
                                .iter()
                                .filter(|r| {
                                    matches!(
                                        r.status,
                                        DeliveryStatus::Failed { .. } | DeliveryStatus::Expired
                                    )
                                })
                                .count();
                            let pending = total - delivered - failed;
                            DeliverySummary {
                                total,
                                delivered,
                                pending,
                                failed,
                            }
                        });

                    let build_engine = |engine: ContactDetailEngine| {
                        let mut e = engine
                            .with_field_notes(field_notes)
                            .with_trust(trust_level, proposal_trusted)
                            .with_reciprocity(reciprocity_status)
                            .with_hidden(is_hidden)
                            .with_imported(is_imported);
                        if let Some(summary) = delivery_summary
                            && summary.total > 0
                        {
                            e = e.with_delivery_summary(summary);
                        }
                        e
                    };

                    match shared_info {
                        Some(info) => {
                            Box::new(build_engine(ContactDetailEngine::with_shared_info(
                                item,
                                fields,
                                info,
                                personal_note,
                            )))
                        }
                        None => Box::new(build_engine(ContactDetailEngine::new(
                            item,
                            fields,
                            personal_note,
                        ))),
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
            AppScreen::VerifyFingerprint { contact_id } => {
                let contact = vauchi.get_contact(contact_id).ok().flatten();
                let their_fp = contact
                    .as_ref()
                    .map(|c| c.fingerprint())
                    .unwrap_or_default();
                let our_fp = vauchi.own_fingerprint().unwrap_or_default();
                let is_verified = contact
                    .as_ref()
                    .map(|c| c.is_fingerprint_verified())
                    .unwrap_or(false);
                Box::new(FingerprintVerifyEngine::new(
                    contact_id,
                    &their_fp,
                    &our_fp,
                    is_verified,
                ))
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
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", c.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                        }),
                    }
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    fn load_delivery_items(vauchi: &Vauchi) -> Vec<DeliveryItem> {
        let records = vauchi
            .storage()
            .get_all_delivery_records()
            .unwrap_or_default();

        // Build contact name lookup for recipient IDs.
        let contacts: HashMap<String, String> = vauchi
            .list_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id().to_string(), c.display_name().to_string()))
            .collect();

        records
            .into_iter()
            .map(|r| {
                let contact_name = contacts
                    .get(&r.recipient_id)
                    .cloned()
                    .unwrap_or_else(|| r.recipient_id.clone());

                let (status, detail, retryable) = match &r.status {
                    vauchi_core::storage::DeliveryStatus::Queued => {
                        (Status::Pending, Some("Queued".into()), false)
                    }
                    vauchi_core::storage::DeliveryStatus::Sent => {
                        (Status::InProgress, Some("Sent to relay".into()), false)
                    }
                    vauchi_core::storage::DeliveryStatus::Stored => {
                        (Status::InProgress, Some("Stored on relay".into()), false)
                    }
                    vauchi_core::storage::DeliveryStatus::Delivered => {
                        (Status::Success, None, false)
                    }
                    vauchi_core::storage::DeliveryStatus::Expired => {
                        (Status::Warning, Some("Expired".into()), true)
                    }
                    vauchi_core::storage::DeliveryStatus::Failed { reason } => {
                        (Status::Failed, Some(reason.clone()), true)
                    }
                    _ => (Status::Pending, None, false),
                };

                DeliveryItem {
                    contact_id: r.recipient_id,
                    contact_name,
                    status,
                    detail,
                    retryable,
                }
            })
            .collect()
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
                id: "ip-privacy".into(),
                question: "How is my IP address protected?".into(),
                answer: Some(
                    "Vauchi uses a self-hosted OHTTP relay that strips your IP \
                     address before requests reach the relay server. For additional \
                     protection you can configure a SOCKS5 proxy in Settings. \
                     Timing obfuscation further prevents traffic correlation."
                        .into(),
                ),
                answer_url: Some("https://docs.vauchi.app/faq/privacy".into()),
                category: "Privacy".into(),
            },
        ]
    }
}
