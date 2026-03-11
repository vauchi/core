// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi<T>`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::api::Vauchi;
use crate::contact_card::{ContactField, FieldType};
use crate::network::Transport;

use super::action::{ActionResult, UserAction};
use super::backup_recovery::BackupRecoveryEngine;
use super::component::{ContactItem, FieldDisplay, UiFieldVisibility};
use super::contact_detail::{ContactDetailEngine, ContactNotFoundEngine, SharedInfoView};
use super::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use super::contact_limit::ContactLimitEngine;
use super::contact_list::ContactListEngine;
use super::contact_merge::{ContactMergeEngine, MergePreview};
use super::contact_visibility::ContactVisibilityEngine;
use super::delivery::DeliveryStatusEngine;
use super::device_linking::DeviceLinkingEngine;
use super::duplicate_detection::DuplicateDetectionEngine;
use super::duress_pin::{DuressConfig, DuressPinEngine};
use super::emergency_shred::EmergencyShredEngine;
use super::engine::WorkflowEngine;
use super::exchange::{ExchangeConfig, ExchangeEngine};
use super::form_dialog::{FormDialogEngine, FormDialogType};
use super::gdpr::GdprEngine;
use super::group_detail::GroupDetailEngine;
use super::groups_list::{GroupInfo, GroupsEngine, GroupsMode};
use super::help::{HelpEngine, HelpItem};
use super::lock_screen::LockScreenEngine;
use super::my_info::{MyInfoEngine, MyInfoGroupTab, MyInfoProgress, OwnFieldInfo};
use super::my_info_entry_detail::{EntryContactInfo, MyInfoEntryDetailEngine};
use super::onboarding::OnboardingEngine;
use super::recovery_status::RecoveryEngine;
use super::screen::ScreenModel;
use super::settings::{SettingsConfig, SettingsEngine};
use super::support::SupportEngine;
use super::sync_status::SyncStatusEngine;
use super::tor_settings::TorSettingsEngine;

/// Top-level screens in the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AppScreen {
    Onboarding,
    MyInfo,
    Contacts,
    ContactDetail {
        contact_id: String,
    },
    ContactEdit {
        contact_id: String,
    },
    ContactVisibility {
        contact_id: String,
    },
    Exchange,
    Settings,
    Help,
    Backup,
    Lock,
    DeviceLinking,
    DuressPin,
    EmergencyShred,
    DeliveryStatus,
    Sync,
    TorSettings,
    Recovery,
    Groups,
    GroupDetail {
        group_id: String,
    },
    Privacy,
    Support,
    FormDialog {
        dialog_type: FormDialogType,
    },
    MyInfoEntryDetail {
        field_id: String,
    },
    ContactDuplicates,
    ContactMerge {
        primary_name: String,
        primary_fields: Vec<String>,
        secondary_name: String,
        secondary_fields: Vec<String>,
    },
    ContactLimit,
}

/// Unified orchestrator for all frontends.
pub struct AppEngine<T: Transport> {
    vauchi: Vauchi<T>,
    screen: AppScreen,
    engine: Box<dyn WorkflowEngine>,
    engine_cache: HashMap<AppScreen, Box<dyn WorkflowEngine>>,
    /// Captured from onboarding TextChanged events for identity persistence.
    pending_display_name: Option<String>,
    /// Navigation history stack for back-button support.
    nav_history: Vec<AppScreen>,
}

impl<T: Transport> AppEngine<T> {
    /// Returns a reference to the inner Vauchi instance.
    pub fn vauchi(&self) -> &Vauchi<T> {
        &self.vauchi
    }

    /// Returns a mutable reference to the inner Vauchi instance.
    pub fn vauchi_mut(&mut self) -> &mut Vauchi<T> {
        &mut self.vauchi
    }

    pub fn new(vauchi: Vauchi<T>) -> Self {
        let screen = if !vauchi.has_identity() {
            AppScreen::Onboarding
        } else if vauchi.is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            AppScreen::MyInfo
        };
        let engine = Self::create_engine(&vauchi, &screen);
        Self {
            vauchi,
            screen,
            engine,
            engine_cache: HashMap::new(),
            pending_display_name: None,
            nav_history: Vec::new(),
        }
    }

    pub fn current_app_screen(&self) -> &AppScreen {
        &self.screen
    }

    pub fn has_identity(&self) -> bool {
        self.vauchi.has_identity()
    }

    pub fn navigate_to(&mut self, screen: AppScreen) -> ScreenModel {
        // Push the current screen to nav history before switching
        self.nav_history.push(self.screen.clone());
        self.navigate_to_internal(screen)
    }

    /// Navigate without pushing to history (used by back-navigation and completion routing).
    fn navigate_to_internal(&mut self, screen: AppScreen) -> ScreenModel {
        // Swap in the new screen, get the old one back
        let old_screen = std::mem::replace(&mut self.screen, screen.clone());

        // Build or restore the engine for the new screen
        let new_engine = self
            .engine_cache
            .remove(&screen)
            .unwrap_or_else(|| Self::create_engine(&self.vauchi, &screen));

        // Swap in the new engine, get the old one back
        let old_engine = std::mem::replace(&mut self.engine, new_engine);

        // Cache the old engine if its screen is cacheable
        if Self::is_cacheable(&old_screen) {
            self.engine_cache.insert(old_screen, old_engine);
        }

        self.engine.current_screen()
    }

    /// Navigate back using the history stack. Falls back to Home if empty.
    pub fn navigate_back(&mut self) -> ScreenModel {
        let target = self.nav_history.pop().unwrap_or(AppScreen::MyInfo);
        self.navigate_to_internal(target)
    }

    /// Screens that should never be cached — always start fresh.
    fn is_cacheable(screen: &AppScreen) -> bool {
        !matches!(
            screen,
            AppScreen::Onboarding | AppScreen::Lock | AppScreen::FormDialog { .. }
        )
    }

    /// Invalidates a cached engine for a specific screen.
    /// Next navigation to this screen will create a fresh engine.
    pub fn invalidate_screen(&mut self, screen: &AppScreen) {
        self.engine_cache.remove(screen);
    }

    /// Invalidates all cached engines. Use after bulk mutations.
    pub fn invalidate_all(&mut self) {
        self.engine_cache.clear();
    }

    /// Returns `true` if the current engine has user-entered data that differs
    /// from the original. Used by frontends to show a "discard changes?" prompt.
    pub fn form_has_data(&self) -> bool {
        let dialog_type = match &self.screen {
            AppScreen::FormDialog { dialog_type } => dialog_type,
            _ => return false,
        };
        let input = match self.engine.collected_input() {
            Some(v) => v,
            None => return false,
        };
        match dialog_type {
            FormDialogType::AddField { .. } => {
                // Format: "type\nnote\nvalue\ngroups"
                let parts: Vec<&str> = input.splitn(4, '\n').collect();
                if parts.len() >= 3 {
                    let note = parts.get(1).unwrap_or(&"").trim();
                    let value = parts.get(2).unwrap_or(&"").trim();
                    !note.is_empty() || !value.is_empty()
                } else {
                    false
                }
            }
            FormDialogType::EditField { current_value, .. } => input != *current_value,
            FormDialogType::EditName { current_name } => input != *current_name,
            FormDialogType::EditRelayUrl { current_url } => input != *current_url,
        }
    }

    /// Returns all groups as (id, name) pairs for UI forms.
    pub fn available_groups(&self) -> Vec<(String, String)> {
        self.vauchi
            .list_groups()
            .unwrap_or_default()
            .into_iter()
            .map(|g| (g.id().to_string(), g.name().to_string()))
            .collect()
    }

    /// Returns top-level navigation screens. Sub-screens (Sync, TorSettings,
    /// Recovery, Groups, Privacy, Support) are reached via `navigate_to`.
    pub fn available_screens(&self) -> Vec<AppScreen> {
        if !self.vauchi.has_identity() {
            return vec![AppScreen::Onboarding];
        }
        vec![
            AppScreen::Exchange,
            AppScreen::MyInfo,
            AppScreen::Contacts,
            AppScreen::Settings,
            AppScreen::Help,
        ]
    }

    fn handle_completion(&mut self) -> ActionResult {
        match &self.screen {
            AppScreen::Onboarding => {
                let name = match self.pending_display_name.take() {
                    Some(n) if !n.trim().is_empty() => n,
                    _ => {
                        return ActionResult::ValidationError {
                            component_id: "display_name".into(),
                            message: "Please enter a display name".into(),
                        };
                    }
                };
                match self.vauchi.create_identity(&name) {
                    Ok(()) => {
                        let screen = self.navigate_to_internal(AppScreen::MyInfo);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to create identity: {e}"),
                    },
                }
            }
            AppScreen::Lock => {
                let pin = match self.engine.collected_input() {
                    Some(p) => p,
                    None => {
                        return ActionResult::ValidationError {
                            component_id: "pin".into(),
                            message: "Please enter your password".into(),
                        };
                    }
                };
                match self.vauchi.authenticate(&pin) {
                    Ok(_auth_mode) => {
                        let screen = self.navigate_to_internal(AppScreen::MyInfo);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(_) => {
                        // Notify lock engine of failed auth so it tracks attempts
                        // and clears the entered PIN.
                        self.engine.handle_action(UserAction::ActionPressed {
                            action_id: "auth_failed".into(),
                        })
                    }
                }
            }
            AppScreen::Exchange => {
                let screen = self.navigate_to_internal(AppScreen::Contacts);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyShred => {
                let screen = self.navigate_to_internal(AppScreen::Onboarding);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::FormDialog { ref dialog_type } => {
                let input = self.engine.collected_input();
                let result = match dialog_type {
                    FormDialogType::EditName { .. } => {
                        let name = input.unwrap_or_default();
                        if name.trim().is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "display_name".into(),
                                message: "Display name cannot be empty".into(),
                            };
                        }
                        self.vauchi.update_display_name(&name)
                    }
                    FormDialogType::EditField { field_id, .. } => {
                        let value = input.unwrap_or_default();
                        match self.vauchi.own_card() {
                            Ok(Some(mut card)) => {
                                if let Err(e) = card.update_field_value(field_id, &value) {
                                    return ActionResult::ShowAlert {
                                        title: "Error".into(),
                                        message: format!("Failed to update field: {e}"),
                                    };
                                }
                                self.vauchi.update_own_card(&card).map(|_| ())
                            }
                            Ok(None) => {
                                return ActionResult::ShowAlert {
                                    title: "Error".into(),
                                    message: "No contact card found".into(),
                                };
                            }
                            Err(e) => Err(e),
                        }
                    }
                    FormDialogType::AddField { .. } => {
                        let raw = input.unwrap_or_default();
                        // Format: type\nnote\nvalue\ngroups
                        let mut lines = raw.splitn(4, '\n');
                        let entry_type = lines.next().unwrap_or("custom").trim();
                        let note = lines.next().unwrap_or("").trim();
                        let value = lines.next().unwrap_or("").trim();
                        let _groups = lines.next().unwrap_or("").trim();
                        if value.is_empty() {
                            return ActionResult::ValidationError {
                                component_id: "field_value".into(),
                                message: "Value cannot be empty".into(),
                            };
                        }
                        let field_type = match entry_type {
                            "phone" => FieldType::Phone,
                            "email" => FieldType::Email,
                            "social" => FieldType::Social,
                            "address" => FieldType::Address,
                            "website" => FieldType::Website,
                            "birthday" => FieldType::Birthday,
                            _ => FieldType::Custom,
                        };
                        // Use note as label if provided, otherwise use type name
                        let label = if note.is_empty() {
                            entry_type
                                .chars()
                                .next()
                                .map(|c| c.to_uppercase().to_string() + &entry_type[1..])
                                .unwrap_or_else(|| "Custom".into())
                        } else {
                            note.to_string()
                        };
                        let field = ContactField::new(field_type, &label, value);
                        let field_id = field.id().to_string();
                        let result = self.vauchi.add_own_field(field);
                        // Apply group visibility from selected groups
                        if result.is_ok() && !_groups.is_empty() {
                            for group_id in _groups.split(',').map(|s| s.trim()) {
                                if !group_id.is_empty() {
                                    let _ = self
                                        .vauchi
                                        .set_group_field_visibility(group_id, &field_id, true);
                                }
                            }
                        }
                        result
                    }
                    FormDialogType::EditRelayUrl { .. } => {
                        // Relay URL is TUI-specific config (Backend), not in Vauchi<T>.
                        // Navigate back; TUI handles save via Backend::set_relay_url.
                        Ok(())
                    }
                };
                match result {
                    Ok(()) => {
                        // Invalidate parent screen cache so it refreshes with updated data
                        if let Some(parent) = self.nav_history.last() {
                            self.engine_cache.remove(parent);
                        }
                        let screen = self.navigate_back();
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("{e}"),
                    },
                }
            }
            _ => {
                let screen = self.navigate_back();
                ActionResult::NavigateTo(screen)
            }
        }
    }

    fn create_engine(vauchi: &Vauchi<T>, screen: &AppScreen) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(OnboardingEngine::new()),
            AppScreen::MyInfo => {
                let progress = MyInfoProgress::default();
                let all_groups = vauchi.list_groups().unwrap_or_default();

                // Build own card fields with visibility info
                let (display_name, own_fields) = if let Ok(Some(card)) = vauchi.own_card() {
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
                } else {
                    (String::new(), Vec::new())
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
            AppScreen::MyInfoEntryDetail { ref field_id } => {
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
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default(),
                    own_qr_data: vauchi.public_id().unwrap_or_default(),
                    available_groups,
                };
                Box::new(ExchangeEngine::new(config))
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
            AppScreen::GroupDetail { group_id } => Box::new(GroupDetailEngine::new(
                group_id.clone(),
                "Group".into(),
                vec![],
            )),
            AppScreen::Privacy => {
                Box::new(GdprEngine::new(None, "No data export requested".into()))
            }
            AppScreen::Support => Box::new(SupportEngine::new()),
            AppScreen::FormDialog { dialog_type } => {
                Box::new(FormDialogEngine::new(dialog_type.clone()))
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
                    let item = ContactItem {
                        id: contact.id().to_string(),
                        name: contact.display_name().to_string(),
                        subtitle: None,
                        avatar_initials: initials(contact.display_name()),
                        status: None,
                        searchable_fields: contact
                            .card()
                            .fields()
                            .iter()
                            .map(|f| f.value().to_string())
                            .collect(),
                    };

                    // Build shared info (my card as seen by this contact)
                    let shared_info = Self::build_shared_info(vauchi, contact_id);

                    match shared_info {
                        Some(info) => {
                            Box::new(ContactDetailEngine::with_shared_info(item, fields, info))
                        }
                        None => Box::new(ContactDetailEngine::new(item, fields)),
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
                            .map(|f| super::component::ToggleItem {
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
                ref primary_name,
                ref primary_fields,
                ref secondary_name,
                ref secondary_fields,
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
    fn build_shared_info(vauchi: &Vauchi<T>, contact_id: &str) -> Option<SharedInfoView> {
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

    fn create_entry_detail_engine(vauchi: &Vauchi<T>, field_id: &str) -> Box<dyn WorkflowEngine> {
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
            groups,
            visible_contacts,
        ))
    }

    fn load_contact_items(vauchi: &Vauchi<T>) -> Vec<ContactItem> {
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
                    ContactItem {
                        id: c.id().to_string(),
                        name: c.display_name().to_string(),
                        subtitle,
                        avatar_initials: initials(c.display_name()),
                        status: None,
                        searchable_fields: fields,
                    }
                })
                .collect(),
            Err(e) => {
                eprintln!("[WARN] Failed to load contacts: {e}");
                vec![]
            }
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

fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase()
}

// INLINE_TEST_REQUIRED: initials() is module-private, cannot be tested from external tests/
#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_single_word() {
        assert_eq!(initials("Alice"), "A");
    }

    #[test]
    fn initials_two_words() {
        assert_eq!(initials("Alice Smith"), "AS");
    }

    #[test]
    fn initials_three_words_takes_first_two() {
        assert_eq!(initials("Alice B Smith"), "AB");
    }

    #[test]
    fn initials_empty_string() {
        assert_eq!(initials(""), "");
    }

    #[test]
    fn initials_unicode() {
        assert_eq!(initials("Ägidius Ölmann"), "ÄÖ");
    }

    #[test]
    fn initials_extra_whitespace() {
        assert_eq!(initials("  Alice   Smith  "), "AS");
    }
}

// INLINE_TEST_REQUIRED: initials() is module-private, cannot be tested from external tests/
#[cfg(test)]
mod proptests {
    use super::initials;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn initials_never_panics(name in "\\PC*") {
            let result = initials(&name);
            // Unicode to_uppercase() can expand a single char to multiple,
            // so we only assert the result is valid UTF-8 (which String guarantees)
            // and that it equals its own uppercase form.
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }

        #[test]
        fn initials_are_uppercase(name in "[a-z]+ [a-z]+") {
            let result = initials(&name);
            prop_assert_eq!(result.clone(), result.to_uppercase());
        }
    }
}

impl<T: Transport> WorkflowEngine for AppEngine<T> {
    fn current_screen(&self) -> ScreenModel {
        self.engine.current_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Capture display name during onboarding for identity persistence
        if self.screen == AppScreen::Onboarding {
            if let UserAction::TextChanged {
                ref component_id,
                ref value,
            } = action
            {
                if component_id == "display_name" {
                    self.pending_display_name = Some(value.clone());
                }
            }
        }

        // Persist settings toggles to Vauchi config so fresh engines
        // pick up the latest values (fixes HIGH-4).
        if self.screen == AppScreen::Settings {
            if let UserAction::SettingsToggled {
                ref component_id,
                ref item_id,
            } = action
            {
                if component_id == "privacy" {
                    let config = self.vauchi.config_mut();
                    match item_id.as_str() {
                        "delivery_receipts" => {
                            config.delivery_receipts_enabled = !config.delivery_receipts_enabled;
                        }
                        "suppress_presence" => {
                            config.suppress_presence = !config.suppress_presence;
                        }
                        _ => {}
                    }
                }
            }
        }

        // Intercept entry detail actions before delegating to engine
        if let AppScreen::MyInfoEntryDetail { ref field_id } = self.screen {
            let field_id = field_id.clone();
            match &action {
                UserAction::ItemToggled {
                    component_id,
                    item_id,
                } if component_id == "group_visibility" => {
                    // Persist group visibility change
                    let group_id = item_id.clone();
                    let engine = self
                        .engine
                        .as_any_mut()
                        .and_then(|a| a.downcast_mut::<MyInfoEntryDetailEngine>());
                    if let Some(engine) = engine {
                        // Find current state and toggle
                        let is_visible = engine
                            .groups
                            .iter()
                            .find(|(gid, _, _)| gid == &group_id)
                            .map(|(_, _, v)| *v)
                            .unwrap_or(false);
                        let new_visible = !is_visible;
                        let _ = self.vauchi.set_group_field_visibility(
                            &group_id,
                            &field_id,
                            new_visible,
                        );
                        // Update engine state
                        if let Some(entry) = engine
                            .groups
                            .iter_mut()
                            .find(|(gid, _, _)| gid == &group_id)
                        {
                            entry.2 = new_visible;
                        }
                        // Rebuild visible contacts
                        let all_groups = self.vauchi.list_groups().unwrap_or_default();
                        let mut visible_contacts = Vec::new();
                        let mut seen = std::collections::HashSet::new();
                        for g in &all_groups {
                            if g.is_field_visible(&field_id) {
                                for cid in g.contacts() {
                                    if seen.insert(cid.to_string()) {
                                        let name = self
                                            .vauchi
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
                        engine.visible_contacts = visible_contacts;
                        // Invalidate MyInfo cache so it refreshes
                        self.engine_cache.remove(&AppScreen::MyInfo);
                        return ActionResult::UpdateScreen(engine.current_screen());
                    }
                }
                UserAction::ActionPressed { action_id } if action_id == "edit" => {
                    // Navigate to EditField form for this field
                    if let Some(engine) = self
                        .engine
                        .as_any()
                        .and_then(|a| a.downcast_ref::<MyInfoEntryDetailEngine>())
                    {
                        let label = engine.label.clone();
                        let value = engine.value.clone();
                        let screen = self.navigate_to(AppScreen::FormDialog {
                            dialog_type: FormDialogType::EditField {
                                field_id: field_id.clone(),
                                field_label: label,
                                current_value: value,
                            },
                        });
                        return ActionResult::NavigateTo(screen);
                    }
                }
                UserAction::ActionPressed { action_id } if action_id == "delete" => {
                    // Delete the field from own card
                    if let Ok(Some(mut card)) = self.vauchi.own_card() {
                        let _ = card.remove_field(&field_id);
                        let _ = self.vauchi.update_own_card(&card);
                    }
                    self.engine_cache.remove(&AppScreen::MyInfo);
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                UserAction::ActionPressed { action_id } if action_id == "back" => {
                    let screen = self.navigate_back();
                    return ActionResult::NavigateTo(screen);
                }
                _ => {}
            }
        }

        let result = self.engine.handle_action(action);
        match result {
            ActionResult::Complete => self.handle_completion(),
            ActionResult::EditContact { contact_id } => {
                let screen = self.navigate_to(AppScreen::ContactEdit { contact_id });
                ActionResult::NavigateTo(screen)
            }
            ActionResult::OpenEntryDetail { field_id } => {
                let screen = self.navigate_to(AppScreen::MyInfoEntryDetail { field_id });
                ActionResult::NavigateTo(screen)
            }
            other => other,
        }
    }
}
