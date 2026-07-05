// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Screen building and engine creation for `AppEngine`.

use std::collections::HashMap;

use super::AppEngine;
use super::AppScreen;
use super::help_catalog;
use crate::ui::activity_log::{ActivityLogEngine, ActivityLogItem};
use crate::ui::backup_recovery::BackupRecoveryEngine;
use crate::ui::change_password::ChangePasswordEngine;
use crate::ui::component::{
    A11y, Field, Item, ListItemAction, ListItemActionKind, Status, UiFieldVisibility, initials,
};
use crate::ui::contact_detail::{ContactNotFoundEngine, SharedInfoView};
use crate::ui::contact_list::IndexedItem;
use crate::ui::decoy_contacts::{DecoyContactItem, DecoyContactsEngine};
use crate::ui::delivery::{DeliveryItem, DeliveryStatusEngine, RetryEntry};
use crate::ui::device_linking::DeviceLinkingEngine;
use crate::ui::device_management::{DeviceListItem, DeviceManagementEngine};
use crate::ui::duress_pin::{DuressConfig, DuressPinEngine};
use crate::ui::emergency_broadcast::EmergencyBroadcastEngine;
use crate::ui::emergency_shred::EmergencyShredEngine;
use crate::ui::engine::WorkflowEngine;
use crate::ui::form_dialog::FormDialogEngine;
use crate::ui::gdpr::GdprEngine;
use crate::ui::group_detail::GroupDetailEngine;
use crate::ui::groups_list::{GroupInfo, GroupsEngine, GroupsMode};
use crate::ui::help::HelpEngine;
use crate::ui::lock_screen::{DEFAULT_LOCK_MAX_ATTEMPTS, LockScreenEngine};
use crate::ui::more::MoreEngine;
use crate::ui::my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, OwnFieldInfo};
use crate::ui::my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
use crate::ui::onboarding::OnboardingEngine;
use crate::ui::places_list::{PlaceSummary, PlacesEngine};
use crate::ui::recovery_claim_review::{
    ClaimContext, Confidence, RecoveryClaimReviewEngine, ReviewMode,
};
use crate::ui::recovery_status::RecoveryEngine;
use crate::ui::settings::{SettingsConfig, SettingsEngine};
use crate::ui::support::SupportEngine;
use crate::ui::sync_status::SyncStatusEngine;
use crate::ui::tag_promotion::{PromotionField, TagPromotionEngine};
use crate::ui::tags_list::{TagSummary, TagsEngine};
use vauchi_core::api::Vauchi;

impl AppEngine {
    // Screen-dispatch factory: each argument is an independent input the
    // per-screen builders read (identity, preview mode, capabilities, transport
    // readiness, render context, pending groups, the Glance QR). Bundling them
    // into a struct would only relocate the argument list for a private factory.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn create_engine(
        vauchi: &Vauchi,
        screen: &AppScreen,
        preview_as: Option<&str>,
        device_capabilities: &vauchi_core::exchange::capability::types::DeviceCapabilities,
        transport_readiness: &vauchi_core::exchange::capability::TransportReadiness,
        render_context: &crate::ui::RenderContext,
        pending_groups: &[String],
        glance_qr: Option<&str>,
    ) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(
                OnboardingEngine::new()
                    .with_help_icons(true)
                    .with_locale(render_context.resolved_locale()),
            ),
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
                        .with_locale(render_context.resolved_locale())
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
            AppScreen::Contacts
            | AppScreen::ContactDetail { .. }
            | AppScreen::ContactVisibility { .. }
            | AppScreen::ContactEdit { .. }
            | AppScreen::ContactDuplicates
            | AppScreen::ArchivedContacts
            | AppScreen::ContactMerge { .. }
            | AppScreen::ContactLimit
            | AppScreen::VerifyFingerprint { .. } => {
                Self::create_contacts_engine(vauchi, screen, render_context)
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
                        let locale = render_context.resolved_locale();
                        vauchi
                            .load_backup_reminder_state()
                            .ok()
                            .and_then(|s| s.last_backup_timestamp)
                            .map(|t| crate::relative_time::format_relative_time(now, t, locale))
                            .unwrap_or_else(|| "Never".to_string())
                    },
                };
                Box::new(SettingsEngine::new(config))
            }
            AppScreen::Exchange
            | AppScreen::DeepLinkConsent { .. }
            | AppScreen::DeepLinkResponder { .. }
            | AppScreen::LinkExchange
            | AppScreen::BleExchange { .. }
            | AppScreen::NfcExchange
            | AppScreen::DirectTransport
            | AppScreen::MultiStageExchange { .. } => Self::create_exchange_engine(
                vauchi,
                screen,
                device_capabilities,
                transport_readiness,
                pending_groups,
                glance_qr,
                render_context.resolved_locale(),
            ),
            AppScreen::Help => Box::new(HelpEngine::new(help_catalog::default_help_items())),
            AppScreen::Backup => Box::new(BackupRecoveryEngine::new(
                None,
                vauchi.has_identity(),
                render_context.resolved_locale(),
            )),
            AppScreen::Lock => Box::new(LockScreenEngine::new(DEFAULT_LOCK_MAX_ATTEMPTS)),
            AppScreen::DeviceLinking => {
                let qr_data = vauchi
                    .generate_device_link()
                    .map(|r| r.data_string)
                    .unwrap_or_default();
                Box::new(
                    DeviceLinkingEngine::new(qr_data).with_locale(render_context.resolved_locale()),
                )
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
                Box::new(
                    DeviceManagementEngine::new(devices)
                        .with_locale(render_context.resolved_locale()),
                )
            }
            AppScreen::DuressPin => {
                // Load ALL contacts as the picker pool (even with no stored
                // settings) so a recipient can be chosen (config-gaps defect 1).
                let available_contacts = Self::picker_contacts(vauchi);
                let (enabled, selected_contact_ids, alert_message, include_location) =
                    match vauchi.load_duress_settings().ok().flatten() {
                        Some(s) => (
                            true,
                            s.alert_contact_ids,
                            s.alert_message,
                            s.include_location,
                        ),
                        None => (false, Vec::new(), String::new(), false),
                    };
                Box::new(DuressPinEngine::new(
                    DuressConfig {
                        enabled,
                        available_contacts,
                        selected_contact_ids,
                        alert_message,
                        include_location,
                    },
                    render_context.resolved_locale(),
                ))
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
            AppScreen::ChangePassword => Box::new(ChangePasswordEngine::new(
                vauchi.is_password_enabled().unwrap_or(false),
            )),
            AppScreen::EmergencyShred => {
                Box::new(EmergencyShredEngine::new(render_context.resolved_locale()))
            }
            AppScreen::EmergencyBroadcast => {
                let config = vauchi.load_emergency_config().ok().flatten();
                Box::new(
                    EmergencyBroadcastEngine::new(config)
                        .with_available_contacts(Self::picker_contacts(vauchi)),
                )
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
                Box::new(
                    SyncStatusEngine::new(relay_url, contact_count, pending)
                        .with_locale(render_context.resolved_locale()),
                )
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
                let mut engine =
                    RecoveryEngine::new(contacts, 3).with_locale(render_context.resolved_locale());
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
            AppScreen::TagPromotion { tag_id } => match vauchi.begin_tag_promotion(tag_id) {
                Ok(draft) => {
                    let fields: Vec<PromotionField> = match vauchi.own_card() {
                        Ok(Some(card)) => card
                            .fields()
                            .iter()
                            .map(|f| PromotionField {
                                field_id: f.id().to_string(),
                                label: f.label().to_string(),
                                value: f.value().to_string(),
                                selected: draft.visible_fields.iter().any(|v| v == f.id()),
                            })
                            .collect(),
                        _ => Vec::new(),
                    };
                    Box::new(TagPromotionEngine::new(
                        draft.tag_id,
                        draft.name,
                        draft.contact_ids.len(),
                        fields,
                    ))
                }
                // Tag vanished between navigation and build — fall back to the
                // generic not-found screen (keeps a Back affordance).
                Err(_) => Box::new(ContactNotFoundEngine::new(tag_id.clone())),
            },
            AppScreen::Places => {
                let places: Vec<PlaceSummary> = vauchi
                    .list_places()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|p| PlaceSummary {
                        id: p.id,
                        name: p.name,
                    })
                    .collect();
                Box::new(PlacesEngine::new(places))
            }
            AppScreen::Tags => {
                let tags: Vec<TagSummary> = vauchi
                    .list_tags()
                    .unwrap_or_default()
                    .into_iter()
                    .map(|t| TagSummary {
                        id: t.id,
                        name: t.name,
                        member_count: t.contact_ids.len(),
                    })
                    .collect();
                Box::new(TagsEngine::new(tags))
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
                    GdprEngine::new(
                        deletion_status,
                        "Active".into(),
                        render_context.resolved_locale(),
                    )
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
            AppScreen::More => Box::new(MoreEngine::new(render_context.resolved_locale())),
            AppScreen::ActivityLog => {
                use crate::notification_types::ActivityLogEntry;
                let now = vauchi.clock().unix_seconds();
                let rows = vauchi
                    .storage()
                    .activity_log()
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
        }
    }

    /// Builds a SharedInfoView for a contact — my fields as visible to them.
    pub(super) fn build_shared_info(vauchi: &Vauchi, contact_id: &str) -> Option<SharedInfoView> {
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

    /// Full contact list as picker `Item`s (the pool duress/emergency pickers
    /// render). a11y omitted: each maps to a ToggleItem whose label labels it.
    fn picker_contacts(vauchi: &Vauchi) -> Vec<Item> {
        vauchi
            .list_contacts()
            .unwrap_or_default()
            .into_iter()
            .map(|c| Item {
                id: c.id().to_string(),
                name: c.display_name().to_string(),
                subtitle: None,
                avatar_initials: initials(c.display_name()),
                status: None,
                actions: vec![],
                a11y: None,
            })
            .collect()
    }

    fn load_delivery_items(vauchi: &Vauchi) -> Vec<DeliveryItem> {
        let records = vauchi
            .storage()
            .deliveries()
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
        let entries = vauchi
            .storage()
            .retries()
            .get_all_retry_entries()
            .unwrap_or_default();

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
