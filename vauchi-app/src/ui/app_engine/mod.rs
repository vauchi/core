// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

mod intercept;
mod navigation;
mod routing;
mod screens;

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use vauchi_core::api::Vauchi;

use super::action::{ActionResult, UserAction};
use super::engine::WorkflowEngine;
use super::screen::ScreenModel;

/// Top-level screens in the application.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
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
    Recovery,
    Groups,
    GroupDetail {
        group_id: String,
    },
    Privacy,
    Support,
    FormDialog {
        dialog_type: super::form_dialog::FormDialogType,
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
    VerifyFingerprint {
        contact_id: String,
    },
    More,
}

impl AppScreen {
    /// Canonical navigation-level string ID for this screen.
    ///
    /// Used by CABI to convert between `AppScreen` and the string IDs
    /// that frontends pass to `navigate_to` / receive from `available_screens`.
    /// Exhaustive — adding a new variant without a mapping is a compile error.
    pub fn screen_id(&self) -> &'static str {
        match self {
            Self::Onboarding => "onboarding",
            Self::MyInfo => "my_info",
            Self::Contacts => "contacts",
            Self::ContactDetail { .. } => "contact_detail",
            Self::ContactEdit { .. } => "contact_edit",
            Self::ContactVisibility { .. } => "contact_visibility",
            Self::Exchange => "exchange",
            Self::Settings => "settings",
            Self::Help => "help",
            Self::Backup => "backup",
            Self::Lock => "lock",
            Self::DeviceLinking => "device_linking",
            Self::DuressPin => "duress_pin",
            Self::EmergencyShred => "emergency_shred",
            Self::DeliveryStatus => "delivery_status",
            Self::Sync => "sync",
            Self::Recovery => "recovery",
            Self::Groups => "groups",
            Self::GroupDetail { .. } => "group_detail",
            Self::Privacy => "privacy",
            Self::Support => "support",
            Self::FormDialog { .. } => "form_dialog",
            Self::MyInfoEntryDetail { .. } => "entry_detail",
            Self::ContactDuplicates => "contact_duplicates",
            Self::ContactMerge { .. } => "contact_merge",
            Self::ContactLimit => "contact_limit",
            Self::VerifyFingerprint { .. } => "verify_fingerprint",
            Self::More => "more",
        }
    }

    /// Parse a navigation-level screen ID string into an `AppScreen`.
    ///
    /// Only handles simple (non-parameterized) screens. Parameterized screens
    /// like `ContactDetail` require additional data and return `None`.
    pub fn from_screen_id(id: &str) -> Option<Self> {
        Some(match id {
            "onboarding" => Self::Onboarding,
            "home" | "my_info" => Self::MyInfo,
            "contacts" => Self::Contacts,
            "exchange" => Self::Exchange,
            "settings" => Self::Settings,
            "help" => Self::Help,
            "backup" => Self::Backup,
            "lock" => Self::Lock,
            "device_linking" => Self::DeviceLinking,
            "duress_pin" => Self::DuressPin,
            "emergency_shred" => Self::EmergencyShred,
            "delivery_status" => Self::DeliveryStatus,
            "sync" => Self::Sync,
            "recovery" => Self::Recovery,
            "groups" => Self::Groups,
            "privacy" => Self::Privacy,
            "support" => Self::Support,
            "contact_duplicates" => Self::ContactDuplicates,
            "contact_limit" => Self::ContactLimit,
            "more" => Self::More,
            _ => return None,
        })
    }
}

/// Unified orchestrator for all frontends.
pub struct AppEngine {
    vauchi: Vauchi,
    screen: AppScreen,
    engine: Box<dyn WorkflowEngine>,
    engine_cache: HashMap<AppScreen, Box<dyn WorkflowEngine>>,
    /// Captured from onboarding TextChanged events for identity persistence.
    pending_display_name: Option<String>,
    /// Navigation history stack for back-button support.
    nav_history: Vec<AppScreen>,
    /// Field pending undo after delete from MyInfoEntryDetail.
    pending_field_undo: Option<(String, vauchi_core::contact_card::ContactField)>,
    /// Cached field type catalog (built once from SocialNetworkRegistry).
    field_catalog: vauchi_core::contact_card::FieldTypeCatalog,
    /// Transient preview-as state — contact ID being previewed (not serialized).
    pub(super) preview_as_contact: Option<String>,
}

impl AppEngine {
    /// Returns a reference to the inner Vauchi instance.
    pub fn vauchi(&self) -> &Vauchi {
        &self.vauchi
    }

    /// Returns a mutable reference to the inner Vauchi instance.
    pub fn vauchi_mut(&mut self) -> &mut Vauchi {
        &mut self.vauchi
    }

    pub fn new(vauchi: Vauchi) -> Self {
        let screen = if !vauchi.has_identity() {
            AppScreen::Onboarding
        } else if vauchi.is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            AppScreen::MyInfo
        };
        let engine = Self::create_engine(&vauchi, &screen, None);
        let registry = vauchi_core::social::SocialNetworkRegistry::with_defaults();
        let field_catalog = vauchi_core::contact_card::FieldTypeCatalog::new(&registry);
        Self {
            vauchi,
            screen,
            engine,
            engine_cache: HashMap::new(),
            pending_display_name: None,
            nav_history: Vec::new(),
            pending_field_undo: None,
            field_catalog,
            preview_as_contact: None,
        }
    }

    /// Enter preview-as mode: show MyInfo as seen by the given contact.
    ///
    /// Sets transient state, invalidates the MyInfo cache, and navigates to MyInfo
    /// in PreviewAs view mode. The state is cleared by handling "exit-preview".
    pub fn preview_as(&mut self, contact_id: String) -> ScreenModel {
        self.preview_as_contact = Some(contact_id);
        self.invalidate_screen(&AppScreen::MyInfo);
        self.navigate_to(AppScreen::MyInfo)
    }

    pub fn current_app_screen(&self) -> &AppScreen {
        &self.screen
    }

    pub fn has_identity(&self) -> bool {
        self.vauchi.has_identity()
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

impl WorkflowEngine for AppEngine {
    fn current_screen(&self) -> ScreenModel {
        self.engine.current_screen()
    }

    fn handle_action(&mut self, action: UserAction) -> ActionResult {
        // Capture display name during onboarding for identity persistence
        if self.screen == AppScreen::Onboarding
            && let UserAction::TextChanged {
                ref component_id,
                ref value,
            } = action
            && component_id == "display_name"
        {
            self.pending_display_name = Some(value.clone());
        }

        self.persist_settings_toggle(&action);

        if let Some(result) = self.intercept_exit_preview(&action) {
            return result;
        }

        if let Some(result) = self.intercept_add_field(&action) {
            return result;
        }

        if let Some(result) = self.intercept_settings_action(&action) {
            return result;
        }

        if let AppScreen::MyInfoEntryDetail { ref field_id } = self.screen {
            let field_id = field_id.clone();
            if let Some(result) = self.intercept_entry_detail_action(&field_id, &action) {
                return result;
            }
        }

        if let AppScreen::ContactDetail { ref contact_id } = self.screen {
            let contact_id = contact_id.clone();
            if let Some(result) = self.intercept_personal_note_change(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_field_note_change(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_proposal_trust_toggle(&contact_id, &action) {
                return result;
            }
            if let Some(result) = self.intercept_hide_toggle(&contact_id, &action) {
                return result;
            }
        }

        if let Some(result) = self.handle_undo(&action) {
            return result;
        }

        let result = self.engine.handle_action(action);
        self.route_result(result)
    }
}
