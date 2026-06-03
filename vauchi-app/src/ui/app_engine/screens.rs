// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen building and engine creation for `AppEngine`.

use std::collections::HashMap;

use super::AppEngine;
use super::AppScreen;
use crate::ui::activity_log::{ActivityLogEngine, ActivityLogItem};
use crate::ui::archived_contacts::ArchivedContactsEngine;
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::change_password::ChangePasswordEngine;
use crate::ui::component::{
    A11y, Field, Item, ListItemAction, ListItemActionKind, Status, UiFieldVisibility, initials,
};
use crate::ui::contact_detail::{
    ContactDetailEngine, ContactNotFoundEngine, DeliverySummary, SharedInfoView,
};
use crate::ui::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use crate::ui::contact_limit::ContactLimitEngine;
use crate::ui::contact_list::{ContactListEngine, IndexedItem};
use crate::ui::contact_merge::{ContactMergeEngine, MergePreview};
use crate::ui::contact_visibility::ContactVisibilityEngine;
use crate::ui::decoy_contacts::{DecoyContactItem, DecoyContactsEngine};
use crate::ui::delivery::{DeliveryItem, DeliveryStatusEngine, RetryEntry};
use crate::ui::device_linking::DeviceLinkingEngine;
use crate::ui::device_management::{DeviceListItem, DeviceManagementEngine};
use crate::ui::duplicate_detection::{DuplicateDetectionEngine, DuplicatePair};
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::emergency_broadcast::EmergencyBroadcastEngine;
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
use crate::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};
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
        render_context: &crate::ui::RenderContext,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(OnboardingEngine::new().with_help_icons(true)),
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
                let (display_name, own_fields, avatar_data) = match vauchi.own_card() {
                    Ok(Some(card)) => {
                        let name = card.display_name().to_string();
                        let avatar = card.avatar().map(|a| a.to_vec());
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
                        (name, fields, avatar)
                    }
                    _ => (String::new(), Vec::new(), None),
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
                let pending_updates = vauchi.pending_update_count().unwrap_or(0);
                let last_sync_seconds = vauchi.last_sync_time();
                let now_seconds = vauchi.clock().unix_seconds();
                Box::new(
                    MyInfoEngine::new(progress)
                        .with_own_card(display_name, own_fields)
                        .with_groups(group_tabs)
                        .with_exchange_prompt(!has_contacts)
                        .with_avatar_data(avatar_data)
                        .with_pending_updates(pending_updates)
                        .with_last_sync_seconds(last_sync_seconds)
                        .with_now_seconds(now_seconds),
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
                            .filter(|c| g.contains_contact(&c.item.id))
                            .map(|c| c.item.id.clone())
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
                let bundled = crate::theme::bundled_themes();
                let available_themes: Vec<crate::ui::component::DropdownOption> = bundled
                    .iter()
                    .map(|t| crate::ui::component::DropdownOption {
                        id: t.id.clone(),
                        label: t.name.clone(),
                    })
                    .collect();
                // S6 of 2026-05-16-settings-storage-by-sensitivity:
                // RenderContext is the single source of truth. When the
                // frontend hasn't pushed a value, render the reserved
                // "follow_system" Dropdown option — ADR-047
                // absence-is-follow-system semantic.
                let theme_id = render_context
                    .theme_id
                    .clone()
                    .unwrap_or_else(|| "follow_system".to_string());
                let available_languages: Vec<crate::ui::component::DropdownOption> =
                    crate::i18n::get_available_locales()
                        .into_iter()
                        .map(|l| {
                            let info = crate::i18n::get_locale_info(l);
                            crate::ui::component::DropdownOption {
                                id: info.code.to_string(),
                                label: info.name.to_string(),
                            }
                        })
                        .collect();
                let language_id = render_context
                    .locale
                    .clone()
                    .unwrap_or_else(|| "follow_system".to_string());
                let config = SettingsConfig {
                    display_name,
                    delivery_receipts_enabled: vauchi.config().delivery_receipts_enabled,
                    suppress_presence: vauchi.config().suppress_presence,
                    contact_added_notifications: vauchi.config().contact_added_notifications,
                    relay_url: vauchi.config().relay.server_url.clone(),
                    device_count: 1,
                    password_set: vauchi.is_password_enabled().unwrap_or(false),
                    theme_id,
                    available_themes,
                    language_id,
                    available_languages,
                    reduce_motion: false,
                    high_contrast: false,
                    large_touch: false,
                    show_help_icons: true,
                    // Core/binding semver. Frontends may eventually pass their
                    // own app version + build hash through SettingsConfig —
                    // until then, every frontend renders the binding semver
                    // (matches the AAR/XCFramework pin and is the most
                    // user-actionable identifier we can ship today).
                    version: env!("CARGO_PKG_VERSION").into(),
                    build: String::new(),
                    sync_status: String::new(),
                    pending_updates: 0,
                    failed_deliveries: 0,
                    debug_mode: false,
                    backup_reminder_frequency: vauchi
                        .load_backup_reminder_state()
                        .map(|s| s.frequency.label().to_string())
                        .unwrap_or_else(|_| "Weekly".to_string()),
                    last_backup_display: {
                        let now = vauchi.clock().unix_seconds();
                        vauchi
                            .load_backup_reminder_state()
                            .ok()
                            .and_then(|s| s.last_backup_timestamp)
                            .map(|t| format_relative_time(now, t))
                            .unwrap_or_else(|| "Never".to_string())
                    },
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
                let snapshot_now = vauchi.clock().unix_seconds();
                let card_snapshot = card.as_ref().cloned().map(|c| {
                    vauchi_core::exchange::card_snapshot::CardSnapshot::freeze(c, snapshot_now)
                });
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
                //
                // Site 3 of `2026-05-21-silent-failures-in-security-paths`: the
                // `from_storage_bytes` round-trip is a contract invariant pinned
                // by `identity_storage_bytes_roundtrip_preserves_all_fields` in
                // `core/vauchi-core/tests/it/identity_tests.rs`. A failure here
                // means either a bug in the serializer/parser pair or genuine
                // memory corruption — not a recoverable runtime condition. Pre-
                // 2026-05-23 both sites used `.ok()` and silently dropped the
                // error, so the user tapping "start exchange" got no feedback.
                // We now surface the violation via tracing and keep the
                // graceful-degradation fallback (engine without pre-built
                // session / NFC identity) so the user retains an entry point.
                let session = vauchi
                    .identity()
                    .and_then(reconstruct_identity_via_storage_bytes)
                    .and_then(|identity| {
                        card.map(|c| {
                            let proximity =
                                vauchi_core::exchange::ManualConfirmationVerifier::new();
                            vauchi_core::exchange::ExchangeSession::new_qr(
                                identity,
                                c,
                                proximity,
                                vauchi_core::clock::SystemClock::shared(),
                            )
                        })
                    });

                let nfc_identity = vauchi
                    .identity()
                    .and_then(reconstruct_identity_via_storage_bytes);
                let clock = vauchi.clock().clone();
                let mut engine = match session {
                    Some(s) => ExchangeEngine::with_session(config, s, clock),
                    None => ExchangeEngine::new(config, clock),
                };
                if let Some(id) = nfc_identity {
                    engine.set_nfc_identity(id);
                }
                Box::new(engine)
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
                                vauchi.get_contact(id).ok().flatten().map(|c| Item {
                                    id: c.id().to_string(),
                                    name: c.display_name().to_string(),
                                    subtitle: None,
                                    avatar_initials: initials(c.display_name()),
                                    status: None,
                                    actions: vec![],
                                    a11y: Some(A11y {
                                        label: Some(format!("Contact: {}", c.display_name())),
                                        hint: Some("Double tap to view contact details".into()),
                                        role: None,
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
            AppScreen::DecoyContacts => {
                let decoys: Vec<DecoyContactItem> = vauchi
                    .list_decoy_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(id, display_name, _card)| DecoyContactItem { id, display_name })
                    .collect();
                Box::new(DecoyContactsEngine::new(decoys))
            }
            AppScreen::ChangePassword => Box::new(ChangePasswordEngine::new()),
            AppScreen::EmergencyShred => Box::new(EmergencyShredEngine::new()),
            AppScreen::EmergencyBroadcast => {
                let config = vauchi.load_emergency_config().ok().flatten();
                Box::new(EmergencyBroadcastEngine::new(config))
            }
            AppScreen::DeliveryStatus => {
                let items = Self::load_delivery_items(vauchi);
                let retries = Self::load_retry_entries(vauchi);
                Box::new(DeliveryStatusEngine::new(items).with_retries(retries))
            }
            AppScreen::Sync => {
                let relay_url = vauchi.config().relay.server_url.clone();
                let contact_count = vauchi.list_contacts().map(|c| c.len()).unwrap_or(0);
                let pending = vauchi.pending_update_count().unwrap_or(0) as usize;
                Box::new(SyncStatusEngine::new(relay_url, contact_count, pending))
            }
            AppScreen::Recovery => {
                let contacts: Vec<Item> = Self::load_contact_items(vauchi)
                    .into_iter()
                    .map(|c| c.item)
                    .collect();
                let device_count = vauchi
                    .list_devices()
                    .map(|d| d.len().saturating_sub(1))
                    .unwrap_or(0);
                let mut engine = RecoveryEngine::new(contacts, 3);
                engine.set_linked_device_count(device_count);
                Box::new(engine)
            }
            AppScreen::RecoveryHelp => {
                Box::new(crate::ui::recovery_help::RecoveryHelpEngine::new())
            }
            AppScreen::SocialGraph => {
                use crate::ui::social_graph::{SocialContactEntry, SocialTrustLevel};
                use vauchi_core::contact::TrustLevel;

                let contact_items = Self::load_contact_items(vauchi);
                let entries: Vec<SocialContactEntry> = contact_items
                    .into_iter()
                    .map(|indexed| {
                        let item = indexed.item;
                        let trust_level = vauchi
                            .get_contact(&item.id)
                            .ok()
                            .flatten()
                            .map(|c| match c.trust_level() {
                                TrustLevel::Cautious => SocialTrustLevel::Cautious,
                                TrustLevel::Verified => SocialTrustLevel::Verified,
                                TrustLevel::High => SocialTrustLevel::High,
                                TrustLevel::Standard => SocialTrustLevel::Standard,
                                // TrustLevel is #[non_exhaustive] — default
                                // any future variant to Standard (lowest-trust
                                // bucket) so it surfaces without a warning.
                                _ => SocialTrustLevel::Standard,
                            })
                            .unwrap_or(SocialTrustLevel::Standard);
                        SocialContactEntry {
                            contact: item,
                            trust_level,
                        }
                    })
                    .collect();
                let group_count = vauchi.list_groups().map(|g| g.len()).unwrap_or(0);
                Box::new(crate::ui::SocialGraphEngine::new(entries, group_count))
            }
            AppScreen::Groups => {
                let all_groups = vauchi.list_groups().unwrap_or_default();
                let contacts = Self::load_contact_items(vauchi);
                let group_infos: Vec<GroupInfo> = all_groups
                    .iter()
                    .map(|g| {
                        let member_count = contacts
                            .iter()
                            .filter(|c| g.contains_contact(&c.item.id))
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
                let group = vauchi.get_group(group_id).ok();
                let group_name = group
                    .as_ref()
                    .map(|g| g.name().to_string())
                    .unwrap_or_else(|| "Group".into());
                let mut members: Vec<Item> = vauchi
                    .get_group_members(group_id)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| Item {
                        id: c.id().to_string(),
                        name: c.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(c.display_name()),
                        status: None,
                        actions: vec![],
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", c.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                            role: None,
                        }),
                    })
                    .collect();
                members.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

                // Field visibility — own card fields with their per-group
                // visible flag. Drives the LabelDetail field-toggle UI
                // (Pair 2 of Pure Humble UI retirement).
                let field_visibility: Vec<crate::ui::group_detail::GroupFieldVisibility> =
                    match (vauchi.own_card().ok().flatten(), group.as_ref()) {
                        (Some(card), Some(g)) => card
                            .fields()
                            .iter()
                            .map(|f| crate::ui::group_detail::GroupFieldVisibility {
                                field_id: f.id().to_string(),
                                label: f.label().to_string(),
                                value: f.value().to_string(),
                                is_visible: g.is_field_visible(f.id()),
                            })
                            .collect(),
                        _ => Vec::new(),
                    };

                Box::new(
                    GroupDetailEngine::new(group_id.clone(), group_name, members)
                        .with_field_visibility(field_visibility),
                )
            }
            AppScreen::Privacy => {
                let contact_count = vauchi.contact_count().unwrap_or(0);
                let consent = crate::ui::gdpr::ConsentStatus::from_consent_records(
                    &vauchi.export_consent_log().unwrap_or_default(),
                );
                let state = vauchi_core::api::DeletionManager::new(vauchi.storage())
                    .deletion_state()
                    .unwrap_or(vauchi_core::storage::DeletionState::None);
                let now = vauchi.clock().unix_seconds();
                let (deletion_status, scheduled, executable) = match state {
                    vauchi_core::storage::DeletionState::Scheduled { execute_at, .. } => (
                        Some("Scheduled — cancel within the grace period".to_string()),
                        true,
                        now >= execute_at,
                    ),
                    vauchi_core::storage::DeletionState::Executed { .. } => {
                        (Some("Executed".to_string()), false, false)
                    }
                    vauchi_core::storage::DeletionState::None => (None, false, false),
                    _ => (None, false, false),
                };
                Box::new(
                    GdprEngine::new(deletion_status, "Active".into())
                        .with_deletion_summary(crate::ui::gdpr::DeletionSummary {
                            contact_count,
                            has_backup: false,
                            device_count: 1,
                        })
                        .with_consent(consent)
                        .with_deletion_scheduled(scheduled)
                        .with_deletion_executable(executable),
                )
            }
            AppScreen::Support => Box::new(SupportEngine::new()),
            AppScreen::FormDialog { dialog_type } => {
                Box::new(FormDialogEngine::new(dialog_type.clone()))
            }
            AppScreen::More => Box::new(MoreEngine::new()),
            AppScreen::ActivityLog => {
                use crate::notification_types::ActivityLogEntry;
                let now = vauchi.clock().unix_seconds();
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
                    let fields: Vec<Field> = contact
                        .card()
                        .fields()
                        .iter()
                        .map(|f| {
                            let field_type_str = format!("{:?}", f.field_type());
                            Field {
                                id: f.id().to_string(),
                                icon: crate::ui::component::icon_for_field_type(&field_type_str)
                                    .into(),
                                field_type: field_type_str,
                                label: f.label().to_string(),
                                value: f.value().to_string(),
                                visibility: UiFieldVisibility::Shown,
                                a11y: None,
                            }
                        })
                        .collect();
                    let status = if vauchi.is_contact_revoked(contact.id()) {
                        Some("Deleted their identity".into())
                    } else if contact.has_recovered() && !contact.is_fingerprint_verified() {
                        Some("Recovered — re-verify recommended".into())
                    } else {
                        None
                    };
                    let item = Item {
                        id: contact.id().to_string(),
                        name: contact.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(contact.display_name()),
                        status,
                        actions: vec![],
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", contact.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                            role: None,
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
                    let trust_level_enum = contact.trust_level();
                    let proposal_trusted = contact.is_proposal_trusted();
                    let is_hidden = contact.is_hidden();
                    let is_imported = contact.is_imported();
                    let is_verified = contact.is_fingerprint_verified();
                    let fingerprint = contact.fingerprint();
                    let is_recovery_trusted = contact.is_recovery_trusted();

                    // Reciprocity status (design spec §6.3)
                    use vauchi_core::exchange::reciprocity::Reciprocity;
                    let reciprocity_status = match contact.reciprocity(0) {
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

                    let avatar_data = contact.card().avatar().map(|a| a.to_vec());

                    let build_engine = |engine: ContactDetailEngine| {
                        let mut e = engine
                            .with_avatar_data(avatar_data)
                            .with_field_notes(field_notes)
                            .with_trust(trust_level, proposal_trusted)
                            .with_reciprocity(reciprocity_status)
                            .with_hidden(is_hidden)
                            .with_imported(is_imported)
                            .with_verification(is_verified, trust_level_enum)
                            .with_fingerprint(fingerprint)
                            .with_recovery_trusted(is_recovery_trusted);
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
                                a11y: None,
                                info_key: None,
                            })
                            .collect();
                        (name, items)
                    }
                    _ => (
                        format!("Contact {}", &contact_id[..8.min(contact_id.len())]),
                        vec![],
                    ),
                };
                Box::new(ContactVisibilityEngine::new(name, fields))
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
                    let avatar_data = vauchi
                        .own_card()
                        .ok()
                        .flatten()
                        .and_then(|c| c.avatar().map(|a| a.to_vec()));
                    Box::new(ContactEditEngine::new(editable, vec![]).with_avatar_data(avatar_data))
                }
                _ => Box::new(ContactNotFoundEngine::new(contact_id.clone())),
            },
            AppScreen::ContactDuplicates => {
                let pairs = vauchi.find_duplicates().unwrap_or_default();
                let ui_pairs: Vec<_> = pairs
                    .iter()
                    .map(|p| {
                        let c1 = vauchi.get_contact(&p.id1).ok().flatten();
                        let c2 = vauchi.get_contact(&p.id2).ok().flatten();
                        let name1 = c1
                            .as_ref()
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| p.id1.clone());
                        let name2 = c2
                            .as_ref()
                            .map(|c| c.display_name().to_string())
                            .unwrap_or_else(|| p.id2.clone());
                        // Cross-kind detection drives the merge-vs-delete-imported
                        // routing in intercept; populate even when one side is
                        // missing (treat missing as not-imported, mirrors get_contact
                        // failure path elsewhere).
                        let is_imported_1 = c1.as_ref().map(|c| c.is_imported()).unwrap_or(false);
                        let is_imported_2 = c2.as_ref().map(|c| c.is_imported()).unwrap_or(false);
                        DuplicatePair {
                            id1: p.id1.clone(),
                            name1,
                            is_imported_1,
                            id2: p.id2.clone(),
                            name2,
                            is_imported_2,
                            similarity: p.similarity,
                        }
                    })
                    .collect();
                Box::new(DuplicateDetectionEngine::new(ui_pairs))
            }
            AppScreen::ArchivedContacts => {
                let archived = vauchi
                    .list_archived_contacts()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|c| (c.id().to_string(), c.display_name().to_string()))
                    .collect();
                Box::new(ArchivedContactsEngine::new(archived))
            }
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
            AppScreen::DeviceReplacement => {
                Box::new(crate::ui::device_replacement::DeviceReplacementEngine::new_source())
                // Note: Settings "Set Up New Device" opens as Source (old device side).
                // Onboarding "Transfer from another device" bypasses this engine entirely
                // via ActionResult::StartDeviceLink. PostRestore is created by the backup
                // restore completion handler (future wiring).
            }
            AppScreen::AvatarEditor => {
                let card = vauchi.own_card().ok().flatten();
                let display_name = card
                    .as_ref()
                    .map(|c| c.display_name().to_string())
                    .unwrap_or_default();
                let has_existing_avatar = card.as_ref().is_some_and(|c| c.avatar().is_some());
                Box::new(crate::ui::avatar_editor::AvatarEditorEngine::new(
                    display_name,
                    has_existing_avatar,
                ))
            }
            AppScreen::RecoveryClaimReview => {
                // Default: vouching mode with low confidence placeholder.
                // In production, AppEngine populates context from the scanned
                // claim data before navigating here.
                Box::new(RecoveryClaimReviewEngine::new(
                    ReviewMode::Vouching,
                    ClaimContext {
                        contact_name: "Unknown".into(),
                        old_pk_fingerprint: String::new(),
                        mutual_voucher_count: 0,
                        threshold: 3,
                        confidence: Confidence::Low,
                    },
                ))
            }
            AppScreen::DeepLinkConsent { payload } => {
                Box::new(crate::ui::DeepLinkConsentEngine::new(payload.clone()))
            }
            AppScreen::DeepLinkResponder { payload } => {
                Box::new(crate::ui::LinkResponderEngine::new(payload.clone()))
            }
            AppScreen::LinkExchange => Box::new(crate::ui::LinkExchangeEngine::new()),
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
            AppScreen::MultiStageExchange { mode } => {
                // The cycle-thread session lives in vauchi-platform —
                // the bridge from MultiStageSessionListener callbacks
                // into this engine's `set_state` / `set_qr_payload` /
                // `set_finalized` / `set_session_ended` setters is
                // wired at the platform-binding layer.
                //
                // Phase 1.E of `2026-05-11-hover-graduation-plan.md`
                // made the constructor mode-aware. Hover gets
                // `new_hover()` (front camera + audio-handshake
                // trigger registered); other supported modes (Glance
                // today; Broadcast / TapHoverShake on future
                // graduations) get `new_glance()` (back camera +
                // audio-quiet). The autonomous audio-handshake
                // trigger in `MobileMultiStageSession` is gated on
                // `is_active_engine_multi_stage_hover()` per the
                // 1.C polish commit, so Glance flows never fire
                // spurious audio chrome.
                let engine = match mode {
                    vauchi_core::exchange::mode::ExchangeMode::Hover => {
                        crate::ui::MultiStageExchangeEngine::new_hover()
                    }
                    vauchi_core::exchange::mode::ExchangeMode::TapHoverShake => {
                        crate::ui::MultiStageExchangeEngine::new_tap_hover_shake()
                    }
                    _ => crate::ui::MultiStageExchangeEngine::new_glance(),
                };
                Box::new(engine)
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
        let my_fields: Vec<Field> = own_card
            .fields()
            .iter()
            .map(|f| {
                let is_visible = vauchi
                    .get_effective_field_visibility(contact_id, f.id())
                    .unwrap_or(true);
                let field_type_str = format!("{:?}", f.field_type());
                Field {
                    id: f.id().to_string(),
                    icon: crate::ui::component::icon_for_field_type(&field_type_str).into(),
                    field_type: field_type_str,
                    label: f.label().to_string(),
                    value: f.value().to_string(),
                    visibility: if is_visible {
                        UiFieldVisibility::Shown
                    } else {
                        UiFieldVisibility::Hidden
                    },
                    a11y: None,
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

    pub(super) fn load_contact_items(vauchi: &Vauchi) -> Vec<IndexedItem> {
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
                    let item = Item {
                        id: c.id().to_string(),
                        name: c.display_name().to_string(),
                        subtitle,
                        avatar_initials: initials(c.display_name()),
                        status,
                        actions: contact_row_actions(c.is_imported(), c.is_hidden()),
                        a11y: Some(A11y {
                            label: Some(format!("Contact: {}", c.display_name())),
                            hint: Some("Double tap to view contact details".into()),
                            role: None,
                        }),
                    };
                    IndexedItem::new(item, fields)
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
                    message_id: r.message_id,
                    contact_id: r.recipient_id,
                    contact_name,
                    status,
                    detail,
                    retryable,
                }
            })
            .collect()
    }

    fn load_retry_entries(vauchi: &Vauchi) -> Vec<RetryEntry> {
        let entries = vauchi.storage().get_all_retry_entries().unwrap_or_default();

        let contacts: HashMap<String, String> = vauchi
            .list_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(|c| (c.id().to_string(), c.display_name().to_string()))
            .collect();

        entries
            .into_iter()
            .map(|e| {
                let contact_name = contacts
                    .get(&e.recipient_id)
                    .cloned()
                    .unwrap_or_else(|| e.recipient_id.clone());
                let max_exceeded = e.is_max_attempts_exceeded();
                RetryEntry {
                    message_id: e.message_id,
                    contact_id: e.recipient_id,
                    contact_name,
                    attempt: e.attempt,
                    max_attempts: e.max_attempts,
                    max_exceeded,
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
                answer_url: Some("https://docs.vauchi.app/users/faq#contacts--exchange".into()),
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
                answer_url: Some("https://docs.vauchi.app/users/faq#privacy--security".into()),
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
                answer_url: Some("https://docs.vauchi.app/users/faq#backup--restore".into()),
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
                answer_url: Some("https://docs.vauchi.app/users/faq#identity--account".into()),
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
                answer_url: Some("https://docs.vauchi.app/users/faq#contacts--exchange".into()),
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
                answer_url: Some("https://docs.vauchi.app/users/faq#privacy--security".into()),
                category: "Privacy".into(),
            },
            HelpItem {
                id: "report-issue".into(),
                question: "Report a Bug".into(),
                answer: None,
                answer_url: Some(Self::bug_report_mailto()),
                category: "Support".into(),
            },
            HelpItem {
                id: "feature-idea".into(),
                question: "Suggest an Idea".into(),
                answer: None,
                answer_url: Some(Self::idea_mailto()),
                category: "Support".into(),
            },
            HelpItem {
                id: "known-issues".into(),
                question: "Known Issues".into(),
                answer: None,
                answer_url: Some("https://docs.vauchi.app/users/known-issues".into()),
                category: "Support".into(),
            },
        ]
    }

    fn bug_report_mailto() -> String {
        let version = env!("CARGO_PKG_VERSION");
        let os = std::env::consts::OS;
        let arch = std::env::consts::ARCH;
        let subject = Self::percent_encode(&format!("Bug Report — Vauchi v{version}"));
        let body = Self::percent_encode(&format!(
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
        let subject = Self::percent_encode(&format!("Idea — Vauchi v{version}"));
        let body = Self::percent_encode(
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
}

/// Per-row swipe actions offered on the contact list. Imported contacts
/// get a reversible soft-delete; exchanged ones get archive. Both can
/// be hidden/unhidden.
fn contact_row_actions(is_imported: bool, is_hidden: bool) -> Vec<ListItemAction> {
    let mut actions = Vec::new();
    if is_hidden {
        actions.push(ListItemAction {
            id: "unhide".into(),
            label: "Unhide".into(),
            kind: ListItemActionKind::Unhide,
            destructive: false,
        });
    } else {
        actions.push(ListItemAction {
            id: "hide".into(),
            label: "Hide".into(),
            kind: ListItemActionKind::Hide,
            destructive: false,
        });
    }
    if is_imported {
        actions.push(ListItemAction {
            id: "delete".into(),
            label: "Delete".into(),
            kind: ListItemActionKind::Delete,
            destructive: false,
        });
    } else {
        actions.push(ListItemAction {
            id: "archive".into(),
            label: "Archive".into(),
            kind: ListItemActionKind::Archive,
            destructive: false,
        });
    }
    actions
}

/// Format a Unix timestamp as a human-readable relative time string.
fn format_relative_time(now: u64, timestamp: u64) -> String {
    let delta = now.saturating_sub(timestamp);
    let days = delta / (24 * 60 * 60);
    if days == 0 {
        "Today".to_string()
    } else if days == 1 {
        "Yesterday".to_string()
    } else if days < 7 {
        format!("{days} days ago")
    } else if days < 30 {
        let weeks = days / 7;
        if weeks == 1 {
            "1 week ago".to_string()
        } else {
            format!("{weeks} weeks ago")
        }
    } else {
        let months = days / 30;
        if months == 1 {
            "1 month ago".to_string()
        } else {
            format!("{months} months ago")
        }
    }
}

/// Round-trips an `Identity` reference via `to_storage_bytes` /
/// `from_storage_bytes` to obtain an owned copy. `Identity` deliberately
/// does not implement `Clone` because it contains private key material;
/// the serialization round-trip is the documented clone path.
///
/// The intermediate buffer is wrapped in `zeroize::Zeroizing` to scrub
/// the serialized form when this fn returns.
///
/// Returns `None` only on contract violation: `from_storage_bytes` is
/// guaranteed to accept the output of `to_storage_bytes`
/// (`identity_storage_bytes_roundtrip_preserves_all_fields` in
/// `core/vauchi-core/tests/it/identity_tests.rs` pins this). A failure
/// here therefore means a bug in the serializer/parser pair or memory
/// corruption — surfaced via `tracing::error!` instead of silently
/// dropped (site 3 of
/// `2026-05-21-silent-failures-in-security-paths`). The caller falls
/// through to the existing graceful-degradation path so the user keeps
/// an entry point into the exchange flow rather than getting a hung
/// "tap does nothing" no-op.
fn reconstruct_identity_via_storage_bytes(
    id_ref: &vauchi_core::identity::Identity,
) -> Option<vauchi_core::identity::Identity> {
    let bytes = zeroize::Zeroizing::new(id_ref.to_storage_bytes());
    match vauchi_core::identity::Identity::from_storage_bytes(
        &bytes,
        vauchi_core::clock::SystemClock::shared().unix_seconds(),
    ) {
        Ok(identity) => Some(identity),
        Err(e) => {
            tracing::error!(
                target: "vauchi.ui.app_engine.screens",
                error = %e,
                "Identity round-trip via to_storage_bytes -> from_storage_bytes failed; \
                 falling back to engine without pre-built session. This is a contract \
                 violation — see identity_storage_bytes_roundtrip_preserves_all_fields."
            );
            None
        }
    }
}
