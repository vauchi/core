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
use super::component::ContactItem;
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
                let contacts = Self::load_contact_items(vauchi);
                let has_identity = vauchi.has_identity();
                let progress = HomeProgress {
                    completed_steps: if has_identity { 3 } else { 0 },
                    total_steps: 3,
                };
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
                let config = DuressConfig {
                    enabled: false,
                    alert_contacts: vec![],
                    alert_message: String::new(),
                    include_location: false,
                };
                Box::new(DuressPinEngine::new(config))
            }
            AppScreen::EmergencyShred => Box::new(EmergencyShredEngine::new()),
            AppScreen::DeliveryStatus => Box::new(DeliveryStatusEngine::new(vec![])),
            AppScreen::ContactDetail { .. } | AppScreen::ContactEdit { .. } => {
                Box::new(ContactListEngine::new(vec![]))
            }
        }
    }

    fn load_contact_items(vauchi: &Vauchi<T>) -> Vec<ContactItem> {
        vauchi
            .list_contacts()
            .unwrap_or_default()
            .iter()
            .map(|c| ContactItem {
                id: c.id().to_string(),
                name: c.display_name().to_string(),
                subtitle: None,
                avatar_initials: initials(c.display_name()),
                status: None,
            })
            .collect()
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
