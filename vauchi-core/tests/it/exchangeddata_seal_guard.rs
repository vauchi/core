// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! HR-1 regression guard (problem
//! `2026-06-01-exchangeddata-encapsulation-hardening`).
//!
//! The `Contact` aggregate root guards crypto/trust mutations behind
//! `TrustLevel`/kind preconditions (see `contact/mod.rs`). That seal only
//! holds if the single mutable path into `ExchangedData` —
//! `ContactKind::exchanged_data_mut` — stays crate-internal in production
//! builds, and no public `&mut ContactKind` accessor exists on `Contact`.
//!
//! These checks read source text (cfg-independent) so they fire regardless
//! of which feature set the test binary is compiled with. A failure means
//! someone widened the seal; fix the source, do not relax the guard
//! (CC-21).

use std::fs;
use std::path::Path;

fn read_src(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// `exchanged_data_mut` must be `pub(crate)` in production. A bare `pub fn`
/// form is permitted only directly under a `#[cfg(... test ...)]` gate
/// (test fixtures legitimately mutate). Re-widening the production
/// definition to bare `pub` would let external consumers mutate crypto
/// fields around the `Contact` root.
// @internal — source-invariant guard for HR-1, maps to no Gherkin scenario.
#[test]
fn exchanged_data_mut_stays_crate_internal_in_production() {
    let src = read_src("src/contact/kind.rs");
    let lines: Vec<&str> = src.lines().collect();
    let mut found_production_pub_crate = false;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("pub(crate) fn exchanged_data_mut") {
            found_production_pub_crate = true;
        }
        if trimmed.starts_with("pub fn exchanged_data_mut") {
            let prev = if i > 0 { lines[i - 1].trim_start() } else { "" };
            let test_gated =
                prev.contains("cfg(any(test") || prev.contains("feature = \"testing\"");
            assert!(
                test_gated,
                "HR-1 violation: bare `pub fn exchanged_data_mut` at kind.rs:{} is not test-gated. \
                 The production accessor must be `pub(crate)` (see contact/kind.rs HR-1).",
                i + 1
            );
        }
    }

    assert!(
        found_production_pub_crate,
        "HR-1: expected a `pub(crate) fn exchanged_data_mut` production definition in contact/kind.rs"
    );
}

/// No public accessor on `Contact` may hand out `&mut ContactKind` — that
/// would re-open the seal regardless of `exchanged_data_mut`'s visibility.
// @internal — source-invariant guard for HR-1, maps to no Gherkin scenario.
#[test]
fn no_public_mutable_kind_accessor_on_contact() {
    let src = read_src("src/contact/mod.rs");
    let violations: Vec<String> = src
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let trimmed = line.trim_start();
            let is_kind_mut = trimmed.starts_with("pub fn kind_mut");
            let returns_mut_kind =
                trimmed.starts_with("pub fn") && line.contains("-> &mut ContactKind");
            (is_kind_mut || returns_mut_kind).then(|| format!("mod.rs:{}: {trimmed}", i + 1))
        })
        .collect();

    assert!(
        violations.is_empty(),
        "HR-1 violation: public mutable `ContactKind` accessor(s) re-open the ExchangedData seal: {violations:?}"
    );
}
