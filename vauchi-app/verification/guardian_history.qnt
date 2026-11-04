// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// One successor along a history of decisions (vauchi/private#796, design
// 2026-11-03-pinned-guardian-policy, P11, P19, P23, P25 to P27). Each
// identity's next identity is registered in advance (P25): its hash is in
// every contact's pin and its seed is split among the policy guardians, so
// a recovery has one destination, whichever phone runs the ceremony. A
// decision per identity and policy version chooses between rotating to
// that registered identity and a policy change that replaces it; each is
// single-decree Paxos among that version's policy guardians (quorum
// 2q > n + F, one per group). Crypto is left out (next_identity.spthy
// covers the seed's secrecy).
//
// Decisions:
//   A   Alice under policy v1 (guardians G1): rotate to N1, or policy P
//       (v2, guardians G2, registering N2 instead)
//   A2  Alice under v2, open once A certified P: rotate to N2
//   N1  N1 under its own registered successor N1b (P27), open once A
//       rotated to N1: the phone holding N1 was lost too
// A contact follows the chain: Alice's successor is what A rotated to, or
// after P what A2 rotated to. `alice_has_one_successor` is over everything
// any contact could accept, not over a single decision.
//
// KEYS_DELETED: retiring G1 approval devices delete their keys once A
// certifies P (P23). STOLEN_OLD_KEYS: G1's approval state for A can be
// reset after the handover, standing for undeleted keys stolen and used
// afresh; with KEYS_DELETED that is impossible, without it this is P23's
// accepted exposure for a contact offline since the handover.
//
// Checked (tools as in guardian_succession.qnt; 2026-11-04), in CI:
//   quint test --main=deleted_keys --match='^(history|deleted)_'
//     two phones running the ceremony propose the same rotation; a rotation
//     wins over an interrupted policy change and A2 stays shut; after P,
//     v2 rotates to N2; a second loss rotates N1 to N1b; deleted keys
//     close A
//   quint test --main=stolen_old_keys --match='^(history|stolen)_'
//     with the retired keys stolen, A rotates to N1 after P while A2
//     rotated to N2: two successors (P23's accepted exposure)
//   Each instance's own trace fails in the other instance.
// Each decision's uniqueness is guardian_voting.qnt's inductive proof (two
// values, one forgetful device); the chain's is by construction here: A2
// opens only after A certified P, and N1 only after A certified N1. Not
// shown: an exhaustive check of this model, recovery-guardian approvals
// (P15), offline catch-up (P21), liveness.
module guardian_history {
  const KEYS_DELETED: bool
  const STOLEN_OLD_KEYS: bool

  // Policy v1 and, after P, v2: C1 retires, B3 and C2 join.
  pure val G1 = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  pure val G2 = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B3" -> "B", "C2" -> "C")
  pure val F = 1
  pure val MAX_BALLOT = 2
  pure val DECISIONS = Set("A", "A2", "N1")
  pure val ALL_GUARDIANS = G1.keys().union(G2.keys())

  pure def valuesOf(d: str): Set[str] =
    if (d == "A") Set("N1", "P") else if (d == "A2") Set("N2") else Set("N1b")

  pure def isQuorumOf(policy: str -> str, q: Set[str]): bool = and {
    q.subseteq(policy.keys()),
    policy.keys().map(g => policy.get(g)).forall(gr => q.exists(g => policy.get(g) == gr)),
    2 * q.size() > policy.keys().size() + F,
  }

  type State = {
    promised: (str, str) -> int,
    acceptedBallot: (str, str) -> int,
    acceptedValue: (str, str) -> str,
    reports: (str, int) -> (str -> (int, str)),
    proposed: (str, int) -> str,
    certs: str -> Set[str],
  }

  var s: State

  pure val KEYS = tuples(DECISIONS, ALL_GUARDIANS)
  pure val QUORUMS = ALL_GUARDIANS.powerset().filter(q => isQuorumOf(G1, q) or isQuorumOf(G2, q))

  action init = s' = {
    promised: KEYS.mapBy(_ => 0),
    acceptedBallot: KEYS.mapBy(_ => 0),
    acceptedValue: KEYS.mapBy(_ => ""),
    reports: tuples(DECISIONS, 1.to(MAX_BALLOT)).mapBy(_ => Map()),
    proposed: Map(),
    certs: DECISIONS.mapBy(_ => Set()),
  }

  // Who decides d: A and N1 under v1's guardians (N1 is reached from A),
  // A2 under v2's.
  pure def policyOf(st: State, d: str): str -> str =
    if (d == "A2") G2 else G1

  pure def isOpen(st: State, d: str): bool =
    if (d == "A") not(KEYS_DELETED and st.certs.get("A").contains("P"))
    else if (d == "A2") st.certs.get("A").contains("P")
    else st.certs.get("A").contains("N1")

  action promise(d: str, g: str, b: int): bool = all {
    isOpen(s, d),
    policyOf(s, d).keys().contains(g),
    b > s.promised.get((d, g)),
    s' = {
      ...s,
      promised: s.promised.set((d, g), b),
      reports: s.reports.set((d, b), s.reports.get((d, b)).put(g,
        (s.acceptedBallot.get((d, g)), s.acceptedValue.get((d, g))))),
    },
  }

  action propose(d: str, b: int, q: Set[str], v: str): bool = {
    val reported = q.map(g => s.reports.get((d, b)).get(g)).filter(r => r._1 > 0)
    val highest = reported.fold(0, (m, r) => if (r._1 > m) r._1 else m)
    all {
      isOpen(s, d),
      valuesOf(d).contains(v),
      not(s.proposed.keys().contains((d, b))),
      isQuorumOf(policyOf(s, d), q),
      q.forall(g => s.reports.get((d, b)).keys().contains(g)),
      highest == 0 or reported.exists(r => r._1 == highest and r._2 == v),
      s' = { ...s, proposed: s.proposed.put((d, b), v) },
    }
  }

  action accept(d: str, g: str, b: int): bool = all {
    isOpen(s, d),
    policyOf(s, d).keys().contains(g),
    s.proposed.keys().contains((d, b)),
    b >= s.promised.get((d, g)),
    s' = {
      ...s,
      promised: s.promised.set((d, g), b),
      acceptedBallot: s.acceptedBallot.set((d, g), b),
      acceptedValue: s.acceptedValue.set((d, g), s.proposed.get((d, b))),
    },
  }

  action certify(d: str, q: Set[str], b: int, v: str): bool = all {
    v != "",
    isQuorumOf(policyOf(s, d), q),
    q.forall(g => s.acceptedBallot.get((d, g)) == b and s.acceptedValue.get((d, g)) == v),
    s' = { ...s, certs: s.certs.set(d, s.certs.get(d).union(Set(v))) },
  }

  // Undeleted G1 keys stolen after the handover: their state for A starts
  // afresh, as if they had never voted.
  action stealOldKey(g: str): bool = all {
    STOLEN_OLD_KEYS,
    G1.keys().contains(g),
    s.certs.get("A").contains("P"),
    s' = {
      ...s,
      promised: s.promised.set(("A", g), 0),
      acceptedBallot: s.acceptedBallot.set(("A", g), 0),
      acceptedValue: s.acceptedValue.set(("A", g), ""),
    },
  }

  action step = {
    nondet d = DECISIONS.oneOf()
    nondet g = ALL_GUARDIANS.oneOf()
    nondet b = 1.to(MAX_BALLOT).oneOf()
    nondet q = QUORUMS.oneOf()
    nondet v = Set("N1", "P", "N2", "N1b").oneOf()
    any {
      promise(d, g, b),
      propose(d, b, q, v),
      accept(d, g, b),
      certify(d, q, b, v),
      stealOldKey(g),
    }
  }

  // Each decision certifies at most one value.
  val one_decision_each = DECISIONS.forall(d => s.certs.get(d).size() <= 1)

  // What contacts can accept as Alice's successor: A's phone, or after the
  // handover A2's. P11 across every receiving contact.
  val alice_has_one_successor =
    s.certs.get("A").union(s.certs.get("A2")).filter(v => v != "P").size() <= 1

  // ── Witness traces ──

  pure val Q1 = Set("A1", "A2", "B1", "C1")
  pure val Q1b = Set("A2", "B1", "B2", "C1")
  pure val Q2 = Set("A1", "A2", "B3", "C2")

  // A on ballot 1 certifies v with Q1.
  run certifiedInA(v: str): bool =
    init
      .then(promise("A", "A1", 1)).then(promise("A", "A2", 1))
      .then(promise("A", "B1", 1)).then(promise("A", "C1", 1))
      .then(propose("A", 1, Q1, v))
      .then(accept("A", "A1", 1)).then(accept("A", "A2", 1))
      .then(accept("A", "B1", 1)).then(accept("A", "C1", 1))
      .then(certify("A", Q1, 1, v))

  // `quint test --main=deleted_keys --match='^(history|deleted)_'`

  // Two phones run the ceremony: the second proposer, on ballot 2, hears
  // N1 reported and proposes N1 too; both rebuilt the same identity.
  run history_two_phones_propose_the_same_rotation =
    init
      .then(promise("A", "A1", 1)).then(promise("A", "A2", 1))
      .then(promise("A", "B1", 1)).then(promise("A", "C1", 1))
      .then(propose("A", 1, Q1, "N1"))
      .then(accept("A", "A1", 1)).then(accept("A", "A2", 1))
      .then(promise("A", "A2", 2)).then(promise("A", "B1", 2))
      .then(promise("A", "B2", 2)).then(promise("A", "C1", 2))
      .then(propose("A", 2, Q1b, "N1"))
      .then(accept("A", "A2", 2)).then(accept("A", "B1", 2))
      .then(accept("A", "B2", 2)).then(accept("A", "C1", 2))
      .then(certify("A", Q1b, 2, "N1"))
      .expect(s.certs.get("A") == Set("N1"))

  // A rotation competes with a policy change: P reached A1 only, ballot
  // 2's quorum leaves A1 out and rotates to N1; A2 never opens.
  run history_rotation_wins_over_an_interrupted_policy_change =
    init
      .then(promise("A", "A1", 1)).then(promise("A", "A2", 1))
      .then(promise("A", "B1", 1)).then(promise("A", "C1", 1))
      .then(propose("A", 1, Q1, "P"))
      .then(accept("A", "A1", 1))
      .then(promise("A", "A2", 2)).then(promise("A", "B1", 2))
      .then(promise("A", "B2", 2)).then(promise("A", "C1", 2))
      .then(propose("A", 2, Q1b, "N1"))
      .then(accept("A", "A2", 2)).then(accept("A", "B1", 2))
      .then(accept("A", "B2", 2)).then(accept("A", "C1", 2))
      .then(certify("A", Q1b, 2, "N1"))
      .expect(s.certs.get("A") == Set("N1"))
      .then(promise("A2", "A1", 1))
      .fail()

  // A certified policy change: v2's guardians rotate to N2, its registered
  // identity.
  run history_policy_change_then_rotation_to_n2 =
    certifiedInA("P")
      .then(promise("A2", "A1", 1)).then(promise("A2", "A2", 1))
      .then(promise("A2", "B3", 1)).then(promise("A2", "C2", 1))
      .then(propose("A2", 1, Q2, "N2"))
      .then(accept("A2", "A1", 1)).then(accept("A2", "A2", 1))
      .then(accept("A2", "B3", 1)).then(accept("A2", "C2", 1))
      .then(certify("A2", Q2, 1, "N2"))
      .expect(s.certs.get("A") == Set("P") and s.certs.get("A2") == Set("N2"))

  // The phone holding N1 is lost too: N1's own registered successor N1b
  // (P27) is rotated to, and contacts follow Alice, N1, then N1b.
  run history_lost_again_rotates_to_the_next_registered =
    certifiedInA("N1")
      .then(promise("N1", "A1", 1)).then(promise("N1", "A2", 1))
      .then(promise("N1", "B1", 1)).then(promise("N1", "C1", 1))
      .then(propose("N1", 1, Q1, "N1b"))
      .then(accept("N1", "A1", 1)).then(accept("N1", "A2", 1))
      .then(accept("N1", "B1", 1)).then(accept("N1", "C1", 1))
      .then(certify("N1", Q1, 1, "N1b"))
      .expect(s.certs.get("N1") == Set("N1b") and alice_has_one_successor)

  // With the keys deleted at the handover, A is closed.
  run deleted_keys_close_the_old_decision =
    history_policy_change_then_rotation_to_n2
      .then(promise("A", "A1", 2))
      .fail()

  // `quint test --main=stolen_old_keys --match='^(history|stolen)_'`

  // The retired keys survive and are stolen: G1 rotates to N1 after P while
  // G2 rotated to N2, so a contact offline since the handover follows N1
  // and the others N2. P23's accepted exposure; N1's seed is in the old
  // guardians' pieces, so this also needs a quorum of them.
  run stolen_keys_fork_an_offline_contact =
    history_policy_change_then_rotation_to_n2
      .then(stealOldKey("A1")).then(stealOldKey("A2"))
      .then(stealOldKey("B1")).then(stealOldKey("C1"))
      .then(promise("A", "A1", 2)).then(promise("A", "A2", 2))
      .then(promise("A", "B1", 2)).then(promise("A", "C1", 2))
      .then(propose("A", 2, Q1, "N1"))
      .then(accept("A", "A1", 2)).then(accept("A", "A2", 2))
      .then(accept("A", "B1", 2)).then(accept("A", "C1", 2))
      .then(certify("A", Q1, 2, "N1"))
      .expect(not(alice_has_one_successor))
}
// P23 as decided: retiring keys are deleted at the handover.
module deleted_keys {
  import guardian_history(KEYS_DELETED = true, STOLEN_OLD_KEYS = false).*
}

// P23's accepted exposure: the retired keys survive and are stolen.
module stolen_old_keys {
  import guardian_history(KEYS_DELETED = false, STOLEN_OLD_KEYS = true).*
}
