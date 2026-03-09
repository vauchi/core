// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Top-level application orchestrator.
//!
//! `AppEngine` wraps `Vauchi<T>`, owns the active workflow engine,
//! handles navigation routing, and implements `WorkflowEngine` so
//! frontends see a single uniform interface.

use crate::api::Vauchi;
use crate::network::Transport;

use super::action::{ActionResult, UserAction};
use super::backup_recovery::BackupRecoveryEngine;
use super::component::{ContactItem, FieldDisplay, UiFieldVisibility};
use super::contact_detail::{ContactDetailEngine, ContactNotFoundEngine};
use super::contact_edit::{ContactEditEngine, EditableContact, EditableField};
use super::contact_list::ContactListEngine;
use super::delivery::DeliveryStatusEngine;
use super::device_linking::DeviceLinkingEngine;
use super::duress_pin::{DuressConfig, DuressPinEngine};
use super::emergency_shred::EmergencyShredEngine;
use super::engine::WorkflowEngine;
use super::exchange::{ExchangeConfig, ExchangeEngine};
use super::help::{HelpEngine, HelpItem};
use super::home::{HomeEngine, HomeProgress};
use super::lock_screen::LockScreenEngine;
use super::onboarding::OnboardingEngine;
use super::screen::ScreenModel;
use super::settings::{SettingsConfig, SettingsEngine};

/// Top-level screens in the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppScreen {
    Onboarding,
    Home,
    Contacts,
    ContactDetail { contact_id: String },
    ContactEdit { contact_id: String },
    Exchange,
    Settings,
    Help,
    Backup,
    Lock,
    DeviceLinking,
    DuressPin,
    EmergencyShred,
    DeliveryStatus,
}

/// Unified orchestrator for all frontends.
pub struct AppEngine<T: Transport> {
    vauchi: Vauchi<T>,
    screen: AppScreen,
    engine: Box<dyn WorkflowEngine>,
    /// Captured from onboarding TextChanged events for identity persistence.
    pending_display_name: Option<String>,
}

impl<T: Transport> AppEngine<T> {
    pub fn new(vauchi: Vauchi<T>) -> Self {
        let screen = if !vauchi.has_identity() {
            AppScreen::Onboarding
        } else if vauchi.is_password_enabled().unwrap_or(false) {
            AppScreen::Lock
        } else {
            AppScreen::Home
        };
        let engine = Self::create_engine(&vauchi, &screen);
        Self {
            vauchi,
            screen,
            engine,
            pending_display_name: None,
        }
    }

    pub fn current_app_screen(&self) -> &AppScreen {
        &self.screen
    }

    pub fn has_identity(&self) -> bool {
        self.vauchi.has_identity()
    }

    pub fn navigate_to(&mut self, screen: AppScreen) -> ScreenModel {
        self.screen = screen;
        self.engine = Self::create_engine(&self.vauchi, &self.screen);
        self.engine.current_screen()
    }

    pub fn available_screens(&self) -> Vec<AppScreen> {
        if !self.vauchi.has_identity() {
            return vec![AppScreen::Onboarding];
        }
        vec![
            AppScreen::Home,
            AppScreen::Contacts,
            AppScreen::Exchange,
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
                        let screen = self.navigate_to(AppScreen::Home);
                        ActionResult::NavigateTo(screen)
                    }
                    Err(e) => ActionResult::ShowAlert {
                        title: "Error".into(),
                        message: format!("Failed to create identity: {e}"),
                    },
                }
            }
            AppScreen::Lock => {
                let screen = self.navigate_to(AppScreen::Home);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::Exchange => {
                let screen = self.navigate_to(AppScreen::Contacts);
                ActionResult::NavigateTo(screen)
            }
            AppScreen::EmergencyShred => {
                let screen = self.navigate_to(AppScreen::Onboarding);
                ActionResult::NavigateTo(screen)
            }
            _ => {
                let screen = self.navigate_to(AppScreen::Home);
                ActionResult::NavigateTo(screen)
            }
        }
    }

    fn create_engine(vauchi: &Vauchi<T>, screen: &AppScreen) -> Box<dyn WorkflowEngine> {
        match screen {
            AppScreen::Onboarding => Box::new(OnboardingEngine::new()),
            AppScreen::Home => {
                let mut contacts = Self::load_contact_items(vauchi);
                contacts.truncate(5); // Home shows recent contacts only
                let progress = vauchi
                    .get_setup_progress()
                    .map(|sp| HomeProgress {
                        completed_steps: sp.completed_steps,
                        total_steps: sp.total_steps,
                    })
                    .unwrap_or(HomeProgress {
                        completed_steps: 0,
                        total_steps: 6,
                    });
                Box::new(HomeEngine::new(contacts, progress))
            }
            AppScreen::Contacts => {
                let contacts = Self::load_contact_items(vauchi);
                Box::new(ContactListEngine::new(contacts))
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
                let config = ExchangeConfig {
                    own_name: card
                        .map(|c| c.display_name().to_string())
                        .unwrap_or_default(),
                    own_qr_data: vauchi.public_id().unwrap_or_default(),
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
            AppScreen::ContactDetail { contact_id } => match vauchi.get_contact(contact_id) {
                Ok(Some(contact)) => {
                    let fields = contact
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
                    };
                    Box::new(ContactDetailEngine::new(item, fields))
                }
                _ => Box::new(ContactNotFoundEngine::new(contact_id.clone())),
            },
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
        }
    }

    fn load_contact_items(vauchi: &Vauchi<T>) -> Vec<ContactItem> {
        match vauchi.list_contacts() {
            Ok(contacts) => contacts
                .iter()
                .map(|c| ContactItem {
                    id: c.id().to_string(),
                    name: c.display_name().to_string(),
                    subtitle: None,
                    avatar_initials: initials(c.display_name()),
                    status: None,
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
                answer_url: Some("https://docs.vauchi.app/faq/add-contact".into()),
                category: "Getting Started".into(),
            },
            HelpItem {
                id: "e2e-encryption".into(),
                question: "What is end-to-end encryption?".into(),
                answer_url: Some("https://docs.vauchi.app/faq/e2e".into()),
                category: "Security".into(),
            },
            HelpItem {
                id: "create-backup".into(),
                question: "How do I create a backup?".into(),
                answer_url: Some("https://docs.vauchi.app/faq/backup".into()),
                category: "Getting Started".into(),
            },
            HelpItem {
                id: "recovery".into(),
                question: "How does social recovery work?".into(),
                answer_url: Some("https://docs.vauchi.app/faq/recovery".into()),
                category: "Security".into(),
            },
            HelpItem {
                id: "exchange-qr".into(),
                question: "How do I exchange contact cards?".into(),
                answer_url: Some("https://docs.vauchi.app/faq/exchange".into()),
                category: "Getting Started".into(),
            },
            HelpItem {
                id: "tor-privacy".into(),
                question: "How does Tor routing protect my privacy?".into(),
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

        let result = self.engine.handle_action(action);
        match result {
            ActionResult::Complete => self.handle_completion(),
            other => other,
        }
    }
}
