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
    "can_go_back",
    "current_screen_json",
    "current_tab_id",
    "dispatch_domain_command",
    "handle_action_json",
    "handle_app_backgrounded",
    "handle_deep_link_uri",
    "handle_hardware_event",
    "has_identity",
    "invalidate_all",
    "invalidate_screen_json",
    "navigate_back_json",
    "new",
    "periodic_sync_tick",
    "poll_notifications",
    "set_device_capabilities_json",
    "set_event_listener",
    "set_network_online",
    // Platform-capability injection setter (B7) — same sanctioned class
    // as the other set_* setters: the frontend injects native keychain
    // access post-construction for the crypto-shred DomainCommands.
    "set_platform_keychain",
    "set_render_context_json",
    "sidebar_items",
    "tab_info",
];

// Surplus reached 0 with slice 32l (2026-05-25): retiring
// `current_link_responder_session` — the last non-Humble `pub fn` on
// `PlatformAppEngine` — closed the Track-B push-to-zero queue. The
// `SURPLUS_RATCHET_CEILING` constant + its ratchet test were removed;
// `platform_app_engine_surface_matches_allowlist_strict` is now the live
// strict-equality gate (no longer `#[ignore]`). The slice put the
// responder machine in the AppEngine (vauchi-app) and surfaced its
// commands through `ActionResult::Commands`, so no new binding surface
// was added — `HUMBLE_ALLOWLIST` stays at 25.

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
    // Humble surface; ADR-047 added `set_render_context_json` for the
    // render-context tier (25 → 26); ADR-048 ratified a retirement
    // policy and three retirements followed: `default_screen_json`
    // (26 → 25), `is_network_online` (25 → 24, frontends never need
    // to query reachability because the offline banner is
    // auto-injected into emitted ScreenModels when
    // `network_online == false`), and `drain_pending_notifications`
    // (24 → 23, the UniFFI wrapper had only PAE-contract tests as
    // callers; mobile frontends consume notifications via
    // `poll_notifications`, desktop frontends via the cabi-routed
    // AppEngine peer). ADR-043 Amendment 3 (2026-05-24) then promoted
    // the two Track-B linchpins `dispatch_domain_command` (the typed
    // DomainCommand router the B7 extension-wrapper pattern dispatches
    // through) and `new` (the mandatory boot constructor every binding
    // calls) from the surplus ratchet to the allowlist (23 -> 25): both
    // are permanent binding surface, not retirable legacy debt. If this
    // number changes again, the next ADR amendment (or a retirement
    // passing ADR-048's G1-G5 gates) must precede the edit. Catching the
    // count drift here is cheaper than discovering it during an ADR audit.
    //
    // CoreScreenIdMap rework Tier-0 (2026-05-27) added `can_go_back`
    // (25 -> 26): a genuine binding query that lets the frontend drive
    // its back affordance from core's nav-history instead of inferring
    // it from a frontend-side screen-id map. ADR-043-legitimate (a query,
    // not domain logic); it was the prerequisite for retiring
    // `navigate_to_json` — done in the CoreScreenIdMap rework S5 once both
    // frontends migrated off it (27 -> 26).
    //
    // B7 keychain batch Phase 1a (2026-06-01) added `set_platform_keychain`
    // (26 -> 27): a platform-capability injection setter in the same
    // sanctioned class as `set_device_capabilities_json` / `set_event_listener`
    // — the frontend injects native keychain access post-construction so the
    // crypto-shred `DomainCommand`s (`SoftShred` / `CancelShred`) can reach it.
    // ADR-031-legitimate (platform injection, not domain logic).
    //
    // `current_screen_id` retirement (2026-06-08, 27 -> 26 -> 25 vs the
    // prior 26 baseline) under ADR-048's G1-G5 gates: zero hand-written
    // frontend callers across all 10 consumer repos; the only in-tree
    // callers were `vauchi-platform` integration tests, migrated in the
    // same MR to parse `screen_id` off `current_screen_json` (the seam
    // that already owns the self-heal + screen read). Tab-bar
    // highlighting — its sole documented use — is owned by
    // `current_tab_id`. See
    // `_private/docs/problems/2026-06-08-pae-allowlist-further-shrink/`.
    assert_eq!(
        HUMBLE_ALLOWLIST.len(),
        25,
        "Humble allow-list size drifted from the 25 expected after \
         retiring `current_screen_id` (ADR-048 G1-G5: zero frontend \
         callers, in-tree test callers migrated to `current_screen_json`). \
         Edits to this list require an ADR amendment (ADR-021/043 \
         for the Humble engine framing — incl. Amendment 3 for the \
         linchpin promotions — or ADR-048's G1-G5 gates for \
         retirements)."
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

    // Strict equality — the Track-B push-to-zero ratchet reached 0 with
    // slice 32l (2026-05-25). PlatformAppEngine must now expose ONLY the
    // Humble allow-list surface; any surplus is a regression.
    assert!(
        surplus.is_empty(),
        "PlatformAppEngine exposes {} non-Humble surplus method(s) — the \
         surface must match HUMBLE_ALLOWLIST exactly now that the ratchet \
         is at 0.\n\n\
         If you ADDED a method: don't. Route the work through \
         handle_action_json (UserAction), render it via Component, push \
         commands through ActionResult::Commands, or move it to a \
         stateless vauchi-core free function. See \
         `_private/docs/planning/todo/2026-05-11-pure-functional-core-program-plan.md`.\n\n\
         Current surplus methods:\n{}",
        surplus.len(),
        surplus
            .iter()
            .map(|n| format!("  - {n}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

// @internal
#[test]
fn platform_app_engine_surface_matches_allowlist_strict() {
    // The end-state gate of the pure-functional-core program: zero
    // surplus. Was `#[ignore]`-red through Phases 0–3; promoted to a
    // live gate 2026-05-25 when slice 32l retired the final surplus
    // (`current_link_responder_session`) and the Track-B ratchet hit 0
    // (push-to-zero plan Done criterion #2). Its failure message
    // enumerates any regressed surplus, indexable by Cmd-F.
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

// ── ADR-043 Amendment 2: one screen-driving UniFFI object per binding ──

/// Permitted `#[uniffi::Object]` structs in `core/vauchi-platform/src/`.
///
/// ADR-043 Amendment 2 (2026-05-17) caps the set: at most one
/// screen-driving UniFFI object (`PlatformAppEngine`) per binding;
/// session-shaped peers per ADR-031 (hardware-event-driven, transient,
/// single-protocol-instance) are explicitly enumerated below; the
/// legacy `VauchiPlatform` is allowlisted with a retire-in-Phase-6
/// comment.
///
/// The amendment text (clarification 2) names categories; this list
/// is the operational enumeration the test enforces against. Adding
/// or removing entries here is the ADR-amendment surface: any new
/// `#[uniffi::Object]` in `vauchi-platform/src/` must either match
/// an existing entry or land paired with an ADR amendment update.
const PERMITTED_UNIFFI_OBJECTS: &[&str] = &[
    // ── Screen-driving (ADR-043 Am.2 clarification 1) ──
    "PlatformAppEngine",
    // ── Legacy ──
    // `VauchiPlatform` is the Phase-B legacy facade; the
    // `2026-05-11-pure-functional-core-program-plan.md` Phase 6 /
    // Task 6.3 retires it after the `SURPLUS_RATCHET_CEILING` hits
    // zero. Allowlisted here until then; remove this entry when
    // Phase 6 lands.
    "VauchiPlatform",
];

/// Walk `core/vauchi-platform/src/**.rs` and collect every
/// `pub struct <Name>` whose declaration is preceded (within 3
/// preceding lines) by a `uniffi::Object` derive or attribute.
///
/// Rationale for the 3-line window: rustfmt-formatted source places
/// derives directly above the struct, optionally interleaved with
/// short attributes (`#[non_exhaustive]`, `#[serde(...)]`). Three
/// lines covers the realistic patterns without false positives from
/// `uniffi::Object` references inside doc comments far above.
fn collect_uniffi_object_names(dir: &Path) -> BTreeSet<String> {
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
        let lines: Vec<&str> = source.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            // Only consider top-level `pub struct` declarations
            // (column 0). Nested or indented structs (e.g. inside
            // `#[cfg(test)] mod`) are not UniFFI-exported.
            if !line.starts_with("pub struct ") {
                continue;
            }
            // Look back up to 3 lines for the uniffi::Object marker.
            let start = i.saturating_sub(3);
            let window = lines[start..i].join("\n");
            if !window.contains("uniffi::Object") {
                continue;
            }
            // Extract the struct name (token after "pub struct ").
            let after = &line["pub struct ".len()..];
            let name: String = after
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                names.insert(name);
            }
        }
    }
    names
}

// @internal
#[test]
fn peer_uniffi_objects_count_matches_allowlist() {
    // Walks `core/vauchi-platform/src/**.rs` and asserts strict-
    // equality between the observed `#[uniffi::Object]` set and
    // PERMITTED_UNIFFI_OBJECTS. Enforces ADR-043 Amendment 2.
    //
    // To add a new UniFFI Object: append to PERMITTED_UNIFFI_OBJECTS
    // **and** update ADR-043's §Amendments section (the rule says
    // additions to the screen-driving slot are forbidden; session
    // peers per ADR-031 require ADR-031 reference, not a carve-out).
    // To remove one (e.g. Phase 6 `VauchiPlatform` retirement):
    // delete from this list when the struct is deleted from
    // vauchi-platform/src/.
    let dir = platform_src_dir();
    assert!(
        dir.is_dir(),
        "could not locate vauchi-platform sources at {}",
        dir.display()
    );
    let observed = collect_uniffi_object_names(&dir);
    let permitted: BTreeSet<String> = PERMITTED_UNIFFI_OBJECTS
        .iter()
        .map(|s| s.to_string())
        .collect();

    let extra: Vec<&String> = observed.difference(&permitted).collect();
    let missing: Vec<&String> = permitted.difference(&observed).collect();

    assert!(
        extra.is_empty(),
        "Unpermitted #[uniffi::Object] structs in core/vauchi-platform/src/: \
         {extra:?}.\n\n\
         ADR-043 Amendment 2 caps the set to one screen-driving object \
         (`PlatformAppEngine`) plus session peers per ADR-031. Adding a \
         new screen-driving peer is forbidden — route through PAE's \
         `handle_action_json` / `current_screen_json` instead. Adding a \
         new session peer requires an ADR-031 reference and an explicit \
         entry in PERMITTED_UNIFFI_OBJECTS in this file.\n\n\
         Full observed set: {observed:?}"
    );
    assert!(
        missing.is_empty(),
        "PERMITTED_UNIFFI_OBJECTS lists names that no longer exist in \
         vauchi-platform/src/: {missing:?}.\n\n\
         A struct was deleted but the allowlist still references it. \
         Remove the dead entry from PERMITTED_UNIFFI_OBJECTS in this \
         file."
    );
}
