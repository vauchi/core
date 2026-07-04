// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Executable Gherkin tests (PI-16).
//!
//! Runs `.feature` files from the sibling `features/` repo against
//! vauchi-core's API via cucumber-rs. Scenarios whose steps are all bound
//! execute and must pass; scenarios with any unbound step are skipped
//! (features are authored ahead of their step definitions). A genuine step
//! failure, parsing error, or hook error fails the process — this is a real
//! regression gate for the wired scenarios, not a green-no-matter-what
//! scaffold — and the skipped count is surfaced (see `main`).
//!
//! Usage:
//!   cargo test --test cucumber_tests
//!   cargo test --test cucumber_tests -- --tags @contact-card

use cucumber::{World, writer::Stats as _};
use vauchi_core::{ContactCard, Vauchi};

mod steps;

/// Shared world state for all cucumber scenarios.
///
/// Each scenario gets a fresh VauchiWorld with identity already created.
#[derive(World)]
#[world(init = Self::new)]
pub struct VauchiWorld {
    pub vauchi: Vauchi,
    pub current_card: Option<ContactCard>,
    pub pending_field_type: Option<String>,
    pub pending_label: Option<String>,
    pub pending_value: Option<String>,
    pub pending_display_name: Option<String>,
    pub pending_password: Option<String>,
    pub backup_data: Option<Vec<u8>>,
    pub last_result: Result<(), String>,
    pub last_error_message: Option<String>,
}

impl std::fmt::Debug for VauchiWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VauchiWorld")
            .field("current_card", &self.current_card)
            .field("pending_field_type", &self.pending_field_type)
            .field("pending_label", &self.pending_label)
            .field("pending_value", &self.pending_value)
            .field("last_result", &self.last_result)
            .field("last_error_message", &self.last_error_message)
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
            pending_password: None,
            backup_data: None,
            last_result: Ok(()),
            last_error_message: None,
        }
    }
}

fn main() {
    // A wired scenario that fails now fails the process; unbound scenarios are
    // skipped and reported below. Deferred (needs features-repo coordination +
    // a pinned features checkout so the count is deterministic): a passing-count
    // ratchet + `@wip`-tagging to also gate silent skip-*regressions*. See
    // `problems/2026-07-04-cucumber-backgrounds-fail-silently`.
    //
    //   cargo test --test cucumber_tests -- --tags @contact-card
    let features_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../features");

    // CI clones the sibling features/ repo before this job; locally it may be
    // absent until `just setup`. Skip gracefully instead of failing.
    if !std::path::Path::new(features_dir).exists() {
        eprintln!(
            "Skipping cucumber tests: features directory not found at {features_dir}. \
             Run `just setup` to clone the features repo."
        );
        return;
    }

    let writer =
        futures::executor::block_on(VauchiWorld::cucumber().with_default_cli().run(features_dir));

    let scenarios = writer.scenarios_stats();
    let executed = scenarios.passed + scenarios.failed;
    eprintln!(
        "cucumber coverage: {executed}/{} scenarios executed \
         ({} passed, {} skipped [unbound steps], {} failed)",
        scenarios.total(),
        scenarios.passed,
        scenarios.skipped,
        scenarios.failed,
    );

    // Skipped (unbound) scenarios are tolerated by design; a genuine failure is
    // a regression in a wired scenario and must fail CI.
    if writer.execution_has_failed() {
        eprintln!(
            "cucumber GATE failed: {} step failure(s), {} parsing error(s), \
             {} hook error(s) in a wired scenario.",
            writer.failed_steps(),
            writer.parsing_errors(),
            writer.hook_errors(),
        );
        std::process::exit(1);
    }
}
