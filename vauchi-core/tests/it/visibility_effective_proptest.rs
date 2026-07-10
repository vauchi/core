// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! CC-13 stateful property test for the **production** visibility resolver
//! `Vauchi::get_effective_field_visibility` (ADR-054 D3 + the 2026-07-10
//! field-centric model).
//!
//! A declarative model of the resolver's priority order
//! (override → contact-group grant → group-assigned⇒closed → unassigned
//! toggle, default hidden) is kept alongside a real `Vauchi`. Random
//! sequences of group / override / toggle operations are applied to both,
//! then the resolver's verdict is asserted equal to the model for every
//! (contact, field). Because both propagation paths filter the wire delta
//! through this exact resolver (G4 fix, `propagation.rs` + `features.rs`),
//! the resolver invariant is the wire-delta invariant: a contact never
//! receives a field the model does not grant.

use std::collections::HashSet;

use proptest::prelude::*;
use vauchi_core::{Contact, ContactCard, SymmetricKey, Vauchi};

const N_CONTACTS: usize = 2;
const N_GROUPS: usize = 2;
const N_FIELDS: usize = 3;

#[derive(Debug, Clone)]
enum Op {
    AddToGroup(usize, usize),      // contact, group
    RemoveFromGroup(usize, usize), // contact, group
    Grant(usize, usize),           // group, field
    Revoke(usize, usize),          // group, field
    Override(usize, usize, bool),  // contact, field, visible
    SetOwnPrivate(usize),          // field — toggle Hidden (explicit Nobody)
    SetOwnPublic(usize),           // field — toggle Visible (explicit Everyone)
}

/// Declarative mirror of the resolver's documented priority order.
struct Model {
    groups: Vec<HashSet<usize>>,       // per contact → group indices
    grants: Vec<HashSet<usize>>,       // per group → field indices
    overrides: Vec<Vec<Option<bool>>>, // [contact][field]
    own_visible: HashSet<usize>,       // fields toggled Visible (explicit Everyone)
}

impl Model {
    fn new() -> Self {
        Model {
            groups: vec![HashSet::new(); N_CONTACTS],
            grants: vec![HashSet::new(); N_GROUPS],
            overrides: vec![vec![None; N_FIELDS]; N_CONTACTS],
            own_visible: HashSet::new(),
        }
    }

    /// A field is group-assigned when ANY group grants it, regardless of the
    /// contact under evaluation (field-centric partition, 2026-07-10).
    fn assigned(&self, f: usize) -> bool {
        self.grants.iter().any(|g| g.contains(&f))
    }

    /// Expected effective visibility, mirroring `get_effective_field_visibility`.
    fn effective(&self, c: usize, f: usize) -> bool {
        if let Some(b) = self.overrides[c][f] {
            return b; // Layer C: per-contact override wins
        }
        if self.groups[c].iter().any(|g| self.grants[*g].contains(&f)) {
            return true; // one of the contact's groups grants it
        }
        if self.assigned(f) {
            return false; // group-assigned; this contact holds no grant
        }
        // Unassigned field: the Visible/Hidden toggle applies to every
        // contact alike; unruled defaults to hidden. (The contacts are
        // exchanged, so the legacy per-contact rules stay permissive and
        // only the toggle shapes the verdict here.)
        self.own_visible.contains(&f)
    }
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (0..N_CONTACTS, 0..N_GROUPS).prop_map(|(c, g)| Op::AddToGroup(c, g)),
        (0..N_CONTACTS, 0..N_GROUPS).prop_map(|(c, g)| Op::RemoveFromGroup(c, g)),
        (0..N_GROUPS, 0..N_FIELDS).prop_map(|(g, f)| Op::Grant(g, f)),
        (0..N_GROUPS, 0..N_FIELDS).prop_map(|(g, f)| Op::Revoke(g, f)),
        (0..N_CONTACTS, 0..N_FIELDS, any::<bool>()).prop_map(|(c, f, b)| Op::Override(c, f, b)),
        (0..N_FIELDS).prop_map(Op::SetOwnPrivate),
        (0..N_FIELDS).prop_map(Op::SetOwnPublic),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    // @scenario: visibility_control :: The effective resolver never grants an ungranted field
    #[test]
    fn effective_visibility_matches_model_over_random_ops(
        ops in prop::collection::vec(op_strategy(), 0..40),
    ) {
        let mut wb = Vauchi::in_memory().unwrap();
        wb.create_identity("Alice").unwrap();

        // Two exchanged contacts (fixed keys → no per-case keygen churn).
        let contact_ids: Vec<String> = (0..N_CONTACTS)
            .map(|i| {
                let contact = Contact::from_exchange(
                    [(i as u8) + 1; 32],
                    ContactCard::new(&format!("C{i}")),
                    SymmetricKey::generate(),
                    0,
                );
                let id = contact.id().to_string();
                wb.add_contact(contact).unwrap();
                id
            })
            .collect();
        let group_ids: Vec<String> = (0..N_GROUPS)
            .map(|i| wb.create_group(&format!("G{i}")).unwrap().id().to_string())
            .collect();
        let field_ids: Vec<String> = (0..N_FIELDS).map(|i| format!("f{i}")).collect();

        let mut model = Model::new();
        for op in ops {
            match op {
                Op::AddToGroup(c, g) => {
                    if model.groups[c].insert(g) {
                        wb.add_contact_to_group(&group_ids[g], &contact_ids[c]).unwrap();
                    }
                }
                Op::RemoveFromGroup(c, g) => {
                    if model.groups[c].remove(&g) {
                        wb.remove_contact_from_group(&group_ids[g], &contact_ids[c]).unwrap();
                    }
                }
                Op::Grant(g, f) => {
                    model.grants[g].insert(f);
                    wb.set_group_field_visibility(&group_ids[g], &field_ids[f], true).unwrap();
                }
                Op::Revoke(g, f) => {
                    model.grants[g].remove(&f);
                    wb.set_group_field_visibility(&group_ids[g], &field_ids[f], false).unwrap();
                }
                Op::Override(c, f, b) => {
                    model.overrides[c][f] = Some(b);
                    wb.set_contact_visibility_override(&contact_ids[c], &field_ids[f], b).unwrap();
                }
                Op::SetOwnPrivate(f) => {
                    model.own_visible.remove(&f);
                    wb.set_own_field_private(&field_ids[f]).unwrap();
                }
                Op::SetOwnPublic(f) => {
                    model.own_visible.insert(f);
                    wb.set_own_field_public(&field_ids[f]).unwrap();
                }
            }
        }

        for c in 0..N_CONTACTS {
            for f in 0..N_FIELDS {
                let actual = wb
                    .get_effective_field_visibility(&contact_ids[c], &field_ids[f])
                    .unwrap();
                prop_assert_eq!(
                    actual,
                    model.effective(c, f),
                    "contact {} field {}: resolver disagreed with the model",
                    c,
                    f
                );
                // Security invariant (subsumed by the equality, asserted
                // explicitly): a contact only ever sees a field granted by
                // an override, one of their groups, or the unassigned-field
                // Visible toggle — never a leak.
                if actual {
                    let granted_by_group =
                        model.groups[c].iter().any(|g| model.grants[*g].contains(&f));
                    let granted_by_override = model.overrides[c][f] == Some(true);
                    let granted_by_toggle =
                        !model.assigned(f) && model.own_visible.contains(&f);
                    prop_assert!(
                        granted_by_group || granted_by_override || granted_by_toggle,
                        "contact {} saw ungranted field {} (leak)",
                        c,
                        f
                    );
                }
            }
        }
    }
}
