// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: GPL-3.0-or-later

//! Deterministic key ordering shared by every exchange protocol.
//!
//! Exchange protocols derive roles and transcript orderings from
//! lexicographic byte comparison so both peers reach the same answer
//! without negotiation. The role decision and every sorted transcript
//! encoding must agree on one ordering — a divergence between two
//! inline reimplementations is an interop break, so the rule lives
//! here once. WHY: the ratchet-role rule was previously duplicated
//! per session type and the same role bug had to be fixed twice
//! (2026-05-25).

/// Smaller identity key takes the initiator role.
///
/// Equal keys (self-exchange, rejected upstream) yield `false` on
/// both sides.
pub fn is_initiator(our_identity: &[u8; 32], their_identity: &[u8; 32]) -> bool {
    our_identity < their_identity
}

/// Orders two byte strings lexicographically (smaller first) for
/// transcript encodings both peers must compute identically.
pub fn sorted_pair<'a, T: Ord + ?Sized>(a: &'a T, b: &'a T) -> (&'a T, &'a T) {
    if a <= b { (a, b) } else { (b, a) }
}
