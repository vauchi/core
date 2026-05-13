// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 Stateful property test: random op sequences against a live Vauchi instance.
//!
//! Invariant: `resolve_display_name` never panics and always returns a non-empty string,
//! regardless of which sequence of display-preference mutations has been applied.
//!
//! @scenario: contacts_management.feature - Display name resolution invariants

use proptest::prelude::*;
use vauchi_core::contact::display::{DisplayNamePreference, SharedName, resolve_display_name};
use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

// ── Setup helpers ─────────────────────────────────────────────────────────────

fn setup() -> (Vauchi, String) {
    let mut wb = Vauchi::in_memory().unwrap();
    wb.create_identity("Alice").unwrap();
    let mut pk = [0u8; 32];
    pk[0] = 1;
    let card = ContactCard::new("Bob Default");
    let contact = Contact::from_exchange(pk, card, SymmetricKey::generate(), 0);
    let cid = contact.id().to_string();
    wb.add_contact(contact).unwrap();
    (wb, cid)
}

// ── Op type ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Op {
    SetNickname(String),
    ClearNickname,
    AddSharedName(String, bool),
    RemoveSharedName(String),
    SetPrefPrimary,
    SetPrefCustom,
    SetPrefSharedName(String),
}

// ── Strategies ────────────────────────────────────────────────────────────────

fn name_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("Alice".to_string()),
        Just("Bob".to_string()),
        Just("Carol".to_string()),
        Just("Dave".to_string()),
    ]
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        // Weighted toward state-changing ops most likely to interact
        3 => name_strategy().prop_map(Op::SetNickname),
        1 => Just(Op::ClearNickname),
        3 => (name_strategy(), any::<bool>()).prop_map(|(n, p)| Op::AddSharedName(n, p)),
        2 => name_strategy().prop_map(Op::RemoveSharedName),
        2 => Just(Op::SetPrefPrimary),
        2 => Just(Op::SetPrefCustom),
        2 => name_strategy().prop_map(Op::SetPrefSharedName),
    ]
}

fn ops_strategy() -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(op_strategy(), 0..=20)
}

// ── Apply an op (ignoring errors — state may reject the op) ───────────────────

fn apply_op(wb: &Vauchi, cid: &str, op: &Op) {
    use vauchi_core::DisplayNamePreference as DNP;
    let _ = match op {
        Op::SetNickname(n) => wb.set_contact_nickname(cid, n),
        Op::ClearNickname => wb.clear_contact_nickname(cid),
        Op::AddSharedName(n, p) => wb.add_contact_shared_name(cid, n, *p),
        Op::RemoveSharedName(n) => wb.remove_contact_shared_name(cid, n),
        Op::SetPrefPrimary => wb.set_display_name_preference(cid, DNP::Primary),
        Op::SetPrefCustom => wb.set_display_name_preference(cid, DNP::Custom),
        Op::SetPrefSharedName(n) => {
            wb.set_display_name_preference(cid, DNP::SharedName { name: n.clone() })
        }
    };
}

// ── Stateful property test ─────────────────────────────────────────────────────

proptest! {
    // Each case spins up a full Vauchi instance and applies up to 20 ops —
    // roughly 50ms per case. The default 256 cases push wall-clock to ~12s.
    // 64 cases still cover the op-graph (each op kind appears ~10x with the
    // weighted strategy) while bringing per-test runtime under 3s.
    #![proptest_config(ProptestConfig {
        cases: 64,
        .. ProptestConfig::default()
    })]

    /// After any random sequence of display-preference ops, resolved_display_name
    /// is always non-empty.
    // @internal
    #[test]
    fn resolved_display_name_never_empty_after_random_ops(ops in ops_strategy()) {
        let (wb, cid) = setup();

        for op in &ops {
            apply_op(&wb, &cid, op);
        }

        // Read back current state
        let shared_names: Vec<SharedName> = wb
            .list_contact_shared_names(&cid)
            .expect("list_contact_shared_names must not error");
        let nickname: Option<String> = wb
            .get_contact_nickname(&cid)
            .expect("get_contact_nickname must not error");

        // Resolve using Primary pref — always has a fallback ("Bob Default")
        let result = resolve_display_name(
            "Bob Default",
            &DisplayNamePreference::Primary,
            &shared_names,
            nickname.as_deref(),
        );

        prop_assert!(
            !result.is_empty(),
            "resolve_display_name must never return empty string; ops={ops:?}"
        );
    }

    /// resolve_display_name is non-empty for all three preference variants
    /// simultaneously, after random op sequences.
    // @internal
    #[test]
    fn all_preference_variants_are_non_empty_after_ops(ops in ops_strategy()) {
        let (wb, cid) = setup();

        // Seed at least one shared name so SharedName pref has something to find
        let _ = wb.add_contact_shared_name(&cid, "Bob Default", true);

        for op in &ops {
            apply_op(&wb, &cid, op);
        }

        let shared_names: Vec<SharedName> = wb
            .list_contact_shared_names(&cid)
            .expect("list_contact_shared_names must not error");
        let nickname: Option<String> = wb
            .get_contact_nickname(&cid)
            .expect("get_contact_nickname must not error");

        // Build a fallback name from shared names for the SharedName pref test
        let probe_name = shared_names
            .first()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| "Bob Default".to_string());

        for pref in &[
            DisplayNamePreference::Primary,
            DisplayNamePreference::Custom,
            DisplayNamePreference::SharedName {
                name: probe_name.clone(),
            },
        ] {
            let result = resolve_display_name(
                "Bob Default",
                pref,
                &shared_names,
                nickname.as_deref(),
            );
            prop_assert!(
                !result.is_empty(),
                "resolve_display_name must never return empty for pref={pref:?}; ops={ops:?}"
            );
        }
    }
}
