// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// One successor along a history of decisions (vauchi/private#796, design
// 2026-11-03-pinned-guardian-policy, P11, P19, P23). guardian_succession.qnt
// decides one successor; this model chains them: a recovery competes with
// a policy change in one decision, a policy change hands the next decision
// to new guardians, and a recovered phone lost after its certificate is
// itself recovered. Each decision is single-decree Paxos among its own
// policy guardians (quorum 2q > n + F with one per group); crypto is left
// out, as in guardian_succession.qnt.
//
// Decisions:
//   A   Alice under policy v1 (guardians G1): phone X, phone Y, or policy P
//   A2  Alice under policy v2 (guardians G2), open once A certified P: X or Y
//   X   phone X's successor, open once X is certified (the phone was lost
//       after its commitment): Y, under the policy then current
// A contact follows the chain: Alice's successor is A's X or Y, or after P,
// A2's. `alice_has_one_successor` is over everything any contact could
// accept, not over a single decision.
//
// KEYS_DELETED: retiring G1 approval devices delete their keys once A
// certifies P (P23). STOLEN_OLD_KEYS: G1's approval state for A can be
// reset after the handover, standing for undeleted keys stolen and used
// afresh; with KEYS_DELETED that is impossible, without it this is P23's
// accepted exposure for a contact offline since the handover.
//
// Checked (tools as in guardian_succession.qnt):
//   quint test --main=deleted_keys --match='^(history|deleted)_'   (CI)
//     a policy change certified in A hands the next decision to G2, which
//     decides X; an interrupted handover leaves one decision and A2 shut;
//     deleted keys close A; a lost X is recovered again under G2
//   quint test --main=stolen_old_keys --match='^(history|stolen)_' (CI)
//     with the retired keys stolen, A certifies Y after P while A2
//     certifies X: two successors (P23's accepted exposure)
//   Each instance's own trace fails in the other instance.
//   quint run --max-samples=5000 --max-steps=30 --seed=7 (about 22 traces/s):
//     deleted_keys    alice_has_one_successor  no violation found
//     deleted_keys    one_decision_each        no violation found
//     stolen_old_keys alice_has_one_successor  no violation found
//   The last shows the sampling too weak to reach a fork here: the trace
//   proves that fork reachable, so "no violation" under deleted_keys is
//   weak evidence, not a result.
// Not shown: an exhaustive check of either instance; recovery-guardian
// approvals (P15) and the restore-first sequence (P20), offline catch-up
// (P21), approval-device replacement short of a policy change, liveness
// under explicit delivery assumptions.
module guardian_history {
  const KEYS_DELETED: bool
  const STOLEN_OLD_KEYS: bool

  // Policy v1 and, after P, v2: C1 retires, B3 and C2 join.
  pure val G1 = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  pure val G2 = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B3" -> "B", "C2" -> "C")
  pure val F = 1
  pure val MAX_BALLOT = 2
  pure val DECISIONS = Set("A", "A2", "X")
  pure val ALL_GUARDIANS = G1.keys().union(G2.keys())

  pure def valuesOf(d: str): Set[str] =
    if (d == "A") Set("X", "Y", "P") else if (d == "A2") Set("X", "Y") else Set("Y")

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

  // Who decides d: A2 always G2; X under the policy current when X was
  // certified (G2 after a handover, else G1).
  pure def policyOf(st: State, d: str): str -> str =
    if (d == "A") G1
    else if (d == "A2") G2
    else if (st.certs.get("A").contains("P")) G2
    else G1

  pure def isOpen(st: State, d: str): bool =
    if (d == "A") not(KEYS_DELETED and st.certs.get("A").contains("P"))
    else if (d == "A2") st.certs.get("A").contains("P")
    else st.certs.get("A").union(st.certs.get("A2")).contains("X")

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
    nondet v = Set("X", "Y", "P").oneOf()
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
  pure val Q2 = Set("A1", "A2", "B3", "C2")

  // A recovery and a policy change compete in A; once P is certified the
  // old guardians cannot certify X in A, and v2's guardians decide X in A2.
  run history_policy_change_then_recovery =
    init
      .then(promise("A", "A1", 1)).then(promise("A", "A2", 1))
      .then(promise("A", "B1", 1)).then(promise("A", "C1", 1))
      .then(propose("A", 1, Q1, "P"))
      .then(accept("A", "A1", 1)).then(accept("A", "A2", 1))
      .then(accept("A", "B1", 1)).then(accept("A", "C1", 1))
      .then(certify("A", Q1, 1, "P"))
      .then(promise("A2", "A1", 1)).then(promise("A2", "A2", 1))
      .then(promise("A2", "B3", 1)).then(promise("A2", "C2", 1))
      .then(propose("A2", 1, Q2, "X"))
      .then(accept("A2", "A1", 1)).then(accept("A2", "A2", 1))
      .then(accept("A2", "B3", 1)).then(accept("A2", "C2", 1))
      .then(certify("A2", Q2, 1, "X"))
      .expect(s.certs.get("A") == Set("P") and s.certs.get("A2") == Set("X"))

  // An interrupted handover: P reached A1 only; ballot 2's quorum leaves
  // A1 out, reports nothing, and decides X; A2 never opens.
  run history_interrupted_handover_leaves_one_decision =
    init
      .then(promise("A", "A1", 1)).then(promise("A", "A2", 1))
      .then(promise("A", "B1", 1)).then(promise("A", "C1", 1))
      .then(propose("A", 1, Q1, "P"))
      .then(accept("A", "A1", 1))
      .then(promise("A", "A2", 2)).then(promise("A", "B1", 2))
      .then(promise("A", "B2", 2)).then(promise("A", "C1", 2))
      .then(propose("A", 2, Set("A2", "B1", "B2", "C1"), "X"))
      .then(accept("A", "A2", 2)).then(accept("A", "B1", 2))
      .then(accept("A", "B2", 2)).then(accept("A", "C1", 2))
      .then(certify("A", Set("A2", "B1", "B2", "C1"), 2, "X"))
      .expect(s.certs.get("A") == Set("X"))
      .then(promise("A2", "A1", 1))
      .fail()

  // With the retired keys stolen and not deleted, G1 certifies X in A after
  // P while G2 certifies Y in A2: a contact offline since the handover
  // follows X, the others Y. P23's accepted exposure.
  run stolen_keys_fork_an_offline_contact =
    history_policy_change_then_recovery
      .then(stealOldKey("A1")).then(stealOldKey("A2"))
      .then(stealOldKey("B1")).then(stealOldKey("C1"))
      .then(promise("A", "A1", 2)).then(promise("A", "A2", 2))
      .then(promise("A", "B1", 2)).then(promise("A", "C1", 2))
      .then(propose("A", 2, Q1, "Y"))
      .then(accept("A", "A1", 2)).then(accept("A", "A2", 2))
      .then(accept("A", "B1", 2)).then(accept("A", "C1", 2))
      .then(certify("A", Q1, 2, "Y"))
      .expect(not(alice_has_one_successor))

  // With the keys deleted at the handover, A is closed: nothing more can
  // be promised in it.
  run deleted_keys_close_the_old_decision =
    history_policy_change_then_recovery
      .then(promise("A", "A1", 2))
      .fail()

  // Phone X is lost after its certificate: X's own successor is decided,
  // under v2, and contacts follow Alice, X, then Y.
  run history_lost_phone_is_recovered_again =
    history_policy_change_then_recovery
      .then(promise("X", "A1", 1)).then(promise("X", "A2", 1))
      .then(promise("X", "B3", 1)).then(promise("X", "C2", 1))
      .then(propose("X", 1, Q2, "Y"))
      .then(accept("X", "A1", 1)).then(accept("X", "A2", 1))
      .then(accept("X", "B3", 1)).then(accept("X", "C2", 1))
      .then(certify("X", Q2, 1, "Y"))
      .expect(s.certs.get("X") == Set("Y") and alice_has_one_successor)
}

// P23 as decided: retiring keys are deleted at the handover.
module deleted_keys {
  import guardian_history(KEYS_DELETED = true, STOLEN_OLD_KEYS = false).*
}

// P23's accepted exposure: the retired keys survive and are stolen.
module stolen_old_keys {
  import guardian_history(KEYS_DELETED = false, STOLEN_OLD_KEYS = true).*
}
