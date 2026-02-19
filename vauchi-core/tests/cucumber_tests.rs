// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Executable Gherkin tests (PI-16).
//!
//! Runs `.feature` files from the `features/` repo against vauchi-core's API
//! via cucumber-rs. Unbound steps show as "pending" (not failing).
//!
//! Usage:
//!   cargo test --test cucumber_tests
//!   cargo test --test cucumber_tests -- --tags @contact-card

use cucumber::World;
use vauchi_core::network::MockTransport;
use vauchi_core::{ContactCard, Vauchi};

mod steps;

/// Shared world state for all cucumber scenarios.
///
/// Each scenario gets a fresh VauchiWorld with identity already created.
#[derive(World)]
#[world(init = Self::new)]
pub struct VauchiWorld {
    pub vauchi: Vauchi<MockTransport>,
    pub current_card: Option<ContactCard>,
    pub pending_field_type: Option<String>,
    pub pending_label: Option<String>,
    pub pending_value: Option<String>,
    pub pending_display_name: Option<String>,
    pub last_result: Result<(), String>,
}

impl std::fmt::Debug for VauchiWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VauchiWorld")
            .field("current_card", &self.current_card)
            .field("pending_field_type", &self.pending_field_type)
            .field("pending_label", &self.pending_label)
            .field("pending_value", &self.pending_value)
            .field("last_result", &self.last_result)
            .finish()
    }
}

impl VauchiWorld {
    fn new() -> Self {
        let mut vauchi = Vauchi::in_memory().unwrap();
        vauchi.create_identity("TestUser").unwrap();
        VauchiWorld {
            vauchi,
            current_card: None,
            pending_field_type: None,
            pending_label: None,
            pending_value: None,
            pending_display_name: None,
            last_result: Ok(()),
        }
    }
}

fn main() {
    // Run only the contact_card_management feature (first bound file).
    // As more step definitions are added, expand to the full features/ directory.
    let feature_file = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../features/contact_card_management.feature"
    );

    futures::executor::block_on(
        VauchiWorld::cucumber()
            .with_default_cli()
            .run(feature_file),
    );
}
