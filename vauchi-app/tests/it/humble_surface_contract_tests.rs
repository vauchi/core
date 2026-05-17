// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Humble-surface contract test (Phase 0 / Task 0.2 of
//! `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`).
//!
//! Asserts that every `pub fn` inside an `impl PlatformAppEngine { … }`
//! block in `core/vauchi-platform/src/**` either:
//!
//!   (a) appears in `HUMBLE_ALLOWLIST` — the 25-method genuine binding
//!       surface every frontend renders against, or
//!   (b) is one of the `SURPLUS_RATCHET_CEILING` known-legacy methods
//!       still pending retirement in Phase 3 of the program.
//!
//! As legacy methods are absorbed into `handle_action_json`, promoted
//! to `Component` reads, or moved to stateless `vauchi-core` free
//! functions, the ratchet ceiling drops. When it reaches 0, delete the
//! `SURPLUS_RATCHET_CEILING` constant and the ratchet branch below;
//! the test then enforces strict allow-list equality forever.
//!
//! ## Why source-text parsing instead of compile-time reflection?
//!
//! Rust has no method-reflection facility, and `#[uniffi::export]`
//! is a black-box proc-macro from this crate's perspective. A
//! `build.rs` could emit a generated list, but that creates a second
//! source-of-truth that itself needs a contract. Parsing the source
//! files directly keeps a single source of truth (the impl blocks
//! themselves) and gives a clear failure message that names the
//! exact files + methods. The cost is one `std::fs` walk per test
//! invocation — negligible against the rest of the integration
//! suite.
//!
//! ## How to update when the ratchet moves
//!
//! 1. Retire a legacy method (absorb into `handle_action`, move to a
//!    free function, etc.).
//! 2. Run the test — it now fails because actual surplus
//!    < `SURPLUS_RATCHET_CEILING`.
//! 3. Decrement `SURPLUS_RATCHET_CEILING` to match the new count.
//! 4. Commit both changes together (the retirement and the constant
//!    bump) so `git revert` undoes them atomically.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// The 25-method Humble surface — the binding surface every frontend
/// is allowed to depend on per ADR-021 / ADR-043. Source of truth for
/// Phase 0 / Task 0.2 of the pure-functional-core program plan.
///
/// Edits to this list require an ADR amendment (ADR-021/043 or a
/// follow-up). The list is alphabetically sorted to make additions /
/// removals reviewable.
const HUMBLE_ALLOWLIST: &[&str] = &[
    "advance_qr_frame_json",
    "available_screens_json",
    "boot",
    "current_screen_id",
    "current_screen_json",
    "current_tab_id",
    "default_screen_json",
    "drain_pending_notifications",
    "handle_action_json",
    "handle_app_backgrounded",
    "handle_deep_link_uri",
    "handle_hardware_event",
    "has_identity",
    "invalidate_all",
    "invalidate_screen_json",
    "is_network_online",
    "navigate_back_json",
    "navigate_to_json",
    "periodic_sync_tick",
    "poll_notifications",
    "set_device_capabilities_json",
    "set_event_listener",
    "set_network_online",
    "set_render_context_json",
    "sidebar_items",
    "tab_info",
];

/// Surplus = `pub fn`s on `impl PlatformAppEngine` whose name is NOT
/// in `HUMBLE_ALLOWLIST`. The 2026-05-11 baseline measured 38 such
/// methods (see `_private/docs/audit/2026-05-11-legacy-baseline.md`).
/// Every retirement MR must decrement this. When it hits 0, replace
/// the ratchet branch in `enforce_contract` with the strict-equality
/// branch noted alongside it.
const SURPLUS_RATCHET_CEILING: usize = 35;

/// Path resolution: `CARGO_MANIFEST_DIR` for this integration test
/// points at `core/vauchi-app/`. The platform sources live in the
/// sibling crate.
fn platform_src_dir() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
        .join("..")
        .join("vauchi-platform")
        .join("src")
}

/// Collect every `pub fn <name>` inside an `impl PlatformAppEngine { … }`
/// block under `dir`. Uses column-anchored brace counting: rustfmt
/// guarantees `impl …` opens in column 0 and the matching `}` closes
/// in column 0, so we can detect block boundaries without a full
/// parser.
///
/// Mirrors the algorithm in `scripts/scripts/audit-mobile-surface.sh`
/// — the audit script and this contract must agree on the set of
/// pub fns counted, otherwise the ratchet floor and the contract
/// gate would talk past each other.
fn collect_pae_pub_fn_names(dir: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let source =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut in_pae = false;
        for line in source.lines() {
            if line.starts_with("impl PlatformAppEngine {") {
                in_pae = true;
                continue;
            }
            // Hitting another top-level `impl` or top-level `}`
            // closes the current PAE block. Column-0 anchoring keeps
            // this honest against nested `impl`s inside macros.
            if in_pae && line.starts_with("impl ") {
                in_pae = false;
                continue;
            }
            if in_pae && line.starts_with('}') {
                in_pae = false;
                continue;
            }
            if !in_pae {
                continue;
            }
            // Indented pub fn lines only — guards against false hits
            // on string literals or doc-comments that happen to
            // include the text `pub fn `.
            let trimmed = line.trim_start();
            if line == trimmed {
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    names.insert(name);
                }
            }
        }
    }
    names
}

fn classify(names: &BTreeSet<String>) -> (Vec<String>, Vec<String>) {
    let allowed: BTreeSet<&str> = HUMBLE_ALLOWLIST.iter().copied().collect();
    let mut humble = Vec::new();
    let mut surplus = Vec::new();
    for name in names {
        if allowed.contains(name.as_str()) {
            humble.push(name.clone());
        } else {
            surplus.push(name.clone());
        }
    }
    (humble, surplus)
}

// @internal
#[test]
fn humble_allowlist_is_sorted_and_unique() {
    // Compile-time-ish self-check: the allow-list itself stays sorted
    // and free of duplicates. Without this guard, a sloppy paste-merge
    // could silently break the binary-search-friendly ordering and
    // make the diff of future edits hard to review.
    let mut sorted: Vec<&&str> = HUMBLE_ALLOWLIST.iter().collect();
    sorted.sort();
    let original: Vec<&&str> = HUMBLE_ALLOWLIST.iter().collect();
    assert_eq!(
        original, sorted,
        "HUMBLE_ALLOWLIST must stay alphabetically sorted; edit in place."
    );
    let unique: BTreeSet<&&str> = HUMBLE_ALLOWLIST.iter().collect();
    assert_eq!(
        unique.len(),
        HUMBLE_ALLOWLIST.len(),
        "HUMBLE_ALLOWLIST has duplicate entries."
    );
}

// @internal
#[test]
fn humble_allowlist_size_matches_plan() {
    // The plan's Task 0.2 originally named 25 methods as the genuine
    // Humble surface; ADR-047 (Draft) added `set_render_context_json`
    // for the render-context tier, bringing the size to 26. If this
    // number changes again, the next ADR amendment must precede the
    // edit. Catching the count drift here is cheaper than discovering
    // it during an ADR audit.
    assert_eq!(
        HUMBLE_ALLOWLIST.len(),
        26,
        "Humble allow-list size drifted from the 26 expected after \
         ADR-047 added `set_render_context_json`. Edits to this list \
         require an ADR amendment (ADR-021/043, or ADR-047 for the \
         render-context tier)."
    );
}

// @internal
#[test]
fn platform_app_engine_surface_respects_ratchet() {
    let dir = platform_src_dir();
    assert!(
        dir.is_dir(),
        "could not locate vauchi-platform sources at {}",
        dir.display()
    );
    let names = collect_pae_pub_fn_names(&dir);
    assert!(
        !names.is_empty(),
        "no `pub fn` found under any `impl PlatformAppEngine` block — \
         either the parser broke or PAE has been retired entirely. \
         Either way, this test needs updating."
    );

    let (humble, surplus) = classify(&names);

    // Every method on the Humble allow-list MUST appear on the actual
    // engine. A missing one means a binding was deleted before Phase 6
    // updated this list — the deletion is wrong, not the list.
    let humble_set: BTreeSet<&str> = humble.iter().map(String::as_str).collect();
    let missing: Vec<&&str> = HUMBLE_ALLOWLIST
        .iter()
        .filter(|name| !humble_set.contains(*name))
        .collect();
    assert!(
        missing.is_empty(),
        "Humble allow-list names not present on PlatformAppEngine: {missing:?}. \
         A binding required by frontends has been deleted or renamed; revert the \
         deletion or amend ADR-021/043 + this list together."
    );

    // Ratchet branch — current Phase-3 state. Strict equality forces
    // a constant update whenever a method is retired, which keeps the
    // ratchet honest. Replace with `assert!(surplus.is_empty(), …)` and
    // delete `SURPLUS_RATCHET_CEILING` when surplus hits 0.
    if surplus.len() != SURPLUS_RATCHET_CEILING {
        panic!(
            "PlatformAppEngine surplus method count drifted: actual = {}, \
             ratchet ceiling = {}.\n\n\
             If you ADDED a method: don't. Route the work through \
             handle_action_json (UserAction) or render it via Component. \
             See plan Phase 2 (`_private/docs/planning/todo/\
             2026-05-11-pure-functional-core-program-plan.md`).\n\n\
             If you RETIRED a method (good): decrement \
             SURPLUS_RATCHET_CEILING in this file to {} and commit the \
             two changes together so `git revert` undoes them atomically.\n\n\
             Current surplus methods ({} total):\n{}",
            surplus.len(),
            SURPLUS_RATCHET_CEILING,
            surplus.len(),
            surplus.len(),
            surplus
                .iter()
                .map(|n| format!("  - {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
}

// @internal
#[test]
#[ignore = "intentionally red: lists the Phase-3 retirement queue. \
            Run with `cargo test -- --ignored \
            platform_app_engine_surface_matches_allowlist_strict` to see it."]
fn platform_app_engine_surface_matches_allowlist_strict() {
    // The TDD-red companion to the ratchet test. Asserts the end
    // state of the program (zero surplus) and currently fails by
    // design — its failure message is the explicit Phase-3 work
    // list, indexable by Cmd-F. Kept under `#[ignore]` so `just
    // check` stays green; promoted to a non-ignored test in
    // Phase 6 / Task 6.3 once the ratchet reaches 0.
    //
    // Plan Task 0.2 completion criterion: "test red, failure
    // message clearly enumerates the legacy methods". This is that
    // test.
    let dir = platform_src_dir();
    let names = collect_pae_pub_fn_names(&dir);
    let (_humble, surplus) = classify(&names);
    assert!(
        surplus.is_empty(),
        "PlatformAppEngine still exposes {} non-Humble methods. Each \
         one must be absorbed into handle_action_json (UserAction \
         dispatch), rendered as a Component on the matching ScreenModel, \
         or moved to a stateless vauchi-core free function. Plan \
         Phase 3:\n\n{}",
        surplus.len(),
        surplus
            .iter()
            .map(|n| format!("  - {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}
