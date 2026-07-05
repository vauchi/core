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
//! Usage (needs `testing` for the two-party exchange seams; `test-kdf` for
//! fast per-scenario identity creation):
//!   cargo test --test cucumber_tests --features testing,test-kdf
//!   cargo test --test cucumber_tests --features testing,test-kdf -- --tags @contact-card

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
    /// Reusable core vocabulary: named test contacts → contact id.
    pub contacts: std::collections::BTreeMap<String, String>,
    /// Reusable core vocabulary: named visibility groups → label id.
    pub groups: std::collections::BTreeMap<String, String>,
    /// Multi-party vocabulary: named parties (Alice, Bob, …), each a full
    /// Vauchi instance, so two of them can drive a real exchange.
    pub parties: std::collections::BTreeMap<String, Vauchi>,
    /// Per-party own-card snapshots taken before an edit, so a later sync step
    /// can compute the delta (party name → pre-edit card).
    pub old_cards: std::collections::BTreeMap<String, ContactCard>,
    /// In-flight exchange sessions keyed by party name — created when a party
    /// initiates a QR exchange and consumed when the exchange completes.
    pub sessions: std::collections::BTreeMap<String, vauchi_core::exchange::ExchangeSession>,
    /// Humble-UI driver: the AppEngine hosting `self.vauchi` after
    /// `I open the app` (ADR-021 — flows are ScreenModel-testable in core).
    pub engine: Option<vauchi_app::ui::AppEngine>,
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
            contacts: std::collections::BTreeMap::new(),
            groups: std::collections::BTreeMap::new(),
            parties: std::collections::BTreeMap::new(),
            old_cards: std::collections::BTreeMap::new(),
            sessions: std::collections::BTreeMap::new(),
            engine: None,
        }
    }

    /// A named multi-party Vauchi instance (created via `a user "…"`).
    pub fn party(&self, name: &str) -> &Vauchi {
        self.parties
            .get(name)
            .unwrap_or_else(|| panic!("no party named {name:?}"))
    }

    /// Mints a synthetic *exchanged* contact (so `is_exchanged()` holds, as
    /// visibility overrides require) and registers it by name. The public key
    /// is a deterministic distinct filler — visibility tests never do crypto
    /// with it, only address the contact by its derived id.
    pub fn add_test_contact(&mut self, name: &str) -> String {
        use vauchi_core::{Contact, SymmetricKey};
        let mut public_key = [0u8; 32];
        public_key[0] = (self.contacts.len() as u8).wrapping_add(1);
        for (i, b) in name.bytes().enumerate().take(31) {
            public_key[i + 1] = b;
        }
        let card = ContactCard::new(name);
        let contact = Contact::from_exchange(public_key, card, SymmetricKey::generate(), 0);
        let id = contact.id().to_string();
        self.vauchi.add_contact(contact).unwrap();
        self.contacts.insert(name.to_string(), id.clone());
        id
    }

    pub fn contact_id(&self, name: &str) -> String {
        self.contacts
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("no test contact named {name:?}"))
    }

    pub fn group_id(&self, name: &str) -> String {
        self.groups
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("no visibility group named {name:?}"))
    }

    /// Resolves an own-card field label to its generated id.
    pub fn own_field_id(&self, label: &str) -> String {
        self.vauchi
            .own_card()
            .unwrap()
            .unwrap()
            .fields()
            .iter()
            .find(|f| f.label() == label)
            .unwrap_or_else(|| panic!("no own-card field labeled {label:?}"))
            .id()
            .to_string()
    }
}

fn main() {
    // Two gates below: a genuine step failure fails the process, and a coverage
    // FLOOR fails if a previously-wired scenario silently regresses to skipped
    // (a lost step binding). Unbound scenarios are otherwise tolerated and
    // reported. Still deferred: `@wip`-tagging the aspirational features so the
    // exclusion is explicit rather than implicit. See
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

    // Coverage floor: a currently-wired scenario (all steps bound) must not
    // silently drop to skipped — e.g. a renamed or removed step definition. A
    // FLOOR, not an exact count, so the features repo authoring more scenarios
    // only grows `passed` and never flakes this; it trips only when a wired
    // scenario loses its binding. Bump this when you wire more step
    // definitions; if CI reports a drop, a wired scenario lost its binding —
    // investigate the binding, don't just lower the floor.
    const MIN_WIRED_SCENARIOS: usize = 103;
    if scenarios.passed < MIN_WIRED_SCENARIOS {
        eprintln!(
            "cucumber GATE failed: {} wired scenario(s) passed, expected at least \
             {MIN_WIRED_SCENARIOS} — a previously-wired scenario regressed to \
             skipped (lost its step binding).",
            scenarios.passed,
        );
        std::process::exit(1);
    }
}
