// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! `qr_decode` decodes with `rqrr`, whose only crash guard is the clamped
//! `Perspective::map` of the fork in `vauchi/utils/rqrr`; every build from
//! this workspace (the mobile bindings included) gets it through the
//! `[patch.crates-io]` redirect. The lock file must name the fork by tag,
//! or a marginal camera frame aborts the process again. Record:
//! `_private/docs/problems/done/2026-05-25-rqrr-perspective-panic-crashes-qr-scan`.
const LOCK: &str = include_str!("../../../Cargo.lock");

fn rqrr_sources() -> Vec<Option<String>> {
    let mut sources = Vec::new();
    let mut current: Option<Option<String>> = None;
    for line in LOCK
        .lines()
        .map(str::trim)
        .chain(std::iter::once("[[package]]"))
    {
        if line == "[[package]]" {
            if let Some(source) = current.take() {
                sources.push(source);
            }
        } else if line == "name = \"rqrr\"" {
            current = Some(None);
        } else if let (Some(slot), Some(rest)) = (current.as_mut(), line.strip_prefix("source = "))
        {
            *slot = Some(rest.trim_matches('"').to_string());
        }
    }
    sources
}

// @internal
#[test]
fn rqrr_resolves_from_the_fork_by_tag() {
    let sources = rqrr_sources();
    assert_eq!(
        sources.len(),
        1,
        "exactly one rqrr must resolve: {sources:?}"
    );
    let source = sources[0].as_deref().unwrap_or("<path>");
    assert!(
        source.starts_with("git+https://gitlab.com/vauchi/utils/rqrr.git?tag=v0.11.0%2Bvauchi."),
        "rqrr must come from the fork in vauchi/utils/rqrr by tag, got: {source}"
    );
}
