// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// Deciding the one successor of an identity or a guardian policy
// (vauchi/private#796, design 2026-11-03-pinned-guardian-policy, P11, P14,
// P15). Policy guardians approve; a certificate is what a contact accepts.
// Two legitimate proposals compete (the owner's new phone X, then another
// phone Y after X was lost mid-recovery); guardians approve whatever an
// in-person request shows them, so safety must come from the protocol,
// not from their judgement. Crypto is left out: a vote is an approval
// device's signature, unforgeable here by construction.
//
// Policy guardians in three groups (six, or five where the ballots' depth
// needs a smaller instance). A quorum is either one guardian of
// every group (the backup's rule, ONE_PER_GROUP) or more than half of all
// guardians with one in every group (P1). The protocol is either "sign
// once" or ballots: single-decree Paxos, where a guardian's approval device
// keeps the highest ballot it promised and the last (ballot, value) it
// accepted, and a proposer must re-propose the highest value a quorum of
// promises reports. A certificate is a quorum of accepts of one value at
// one ballot; once issued it is held by contacts and never withdrawn.
// ROLLBACK lets a guardian's approval state be restored from before its
// votes (a backup restore or a replacement device without handover).
//
// Checked (Quint 0.33.0; Apalache through `quint verify`, Java 21):
//   quint test --main=<instance> --match=<prefix> (CI, ci/formal-verification.sh)
//     naive_       one per group, sign once: two disjoint quorums certify X and Y
//     sign_once_   P1 majority, sign once: a three-three split stalls for good
//     ballots_     a retry carries the certified value; an interrupted round
//                  is retried and decides without a fork
//     rolled_back_ P1 majority with C1's state restored: X and Y certified
//     tolerant_    P19 with the same restore: one four-guardian promise set
//                  still reports X (one set, not all)
//   quint verify --max-steps=8  --main=naive     --invariant=one_successor     violated
//   quint verify --max-steps=6  --main=sign_once --invariant=can_still_certify violated
//   quint verify --max-steps=10 --main=sign_once --invariant=one_successor     holds
//   quint run --max-samples=200000 --max-steps=25 --seed=7 --invariant=one_successor
//     --main=ballots_rolled_back   violated (the control reaches the fork)
//     --main=tolerant_rolled_back  no violation found
//     --main=ballots               no violation found
// Not yet shown: `quint verify` of ballots, ballots_rolled_back and
// tolerant_rolled_back at the 17 to 21 steps a second round needs ran an
// hour each without a verdict (2026-11-03). Simulation samples traces and
// bounded checks stop at a depth: neither is a proof.
// Not modelled: recovery racing a policy change, membership handover,
// approval-device replacement, offline catch-up, the restore-first
// sequence (vauchi/private#796, note 4153459959).
module guardian_succession {
  const ONE_PER_GROUP: bool
  const BALLOTS: bool
  const ROLLBACK: bool
  // Equivocating approval devices tolerated: a quorum needs 2q > n + F
  // (P19 sets F = 1; P1's bare majority is F = 0).
  const F: int
  // The guardians whose approval state may be restored from before their
  // votes (a cloned or backup-restored approval device).
  const FAULTY: Set[str]
  // Each policy guardian's group.
  const GROUP_OF: str -> str

  pure val GUARDIANS = GROUP_OF.keys()
  pure val GROUPS = GUARDIANS.map(g => GROUP_OF.get(g))
  pure def groupOf(g: str): str = GROUP_OF.get(g)
  pure val VALUES = Set("X", "Y")
  pure val MAX_BALLOT = 2

  pure def covers(q: Set[str]): bool = GROUPS.forall(gr => q.exists(g => groupOf(g) == gr))
  pure def isQuorum(q: Set[str]): bool =
    if (ONE_PER_GROUP) covers(q) else covers(q) and 2 * q.size() > GUARDIANS.size() + F
  pure val QUORUMS = GUARDIANS.powerset().filter(q => isQuorum(q))

  type State = {
    // Approval-device state, per guardian.
    promised: str -> int,
    acceptedBallot: str -> int,
    acceptedValue: str -> str,
    // What each guardian reported when promising ballot b: its accepted
    // (ballot, value) then.
    reports: int -> (str -> (int, str)),
    // The value the one proposer of ballot b proposed, once it did.
    proposed: int -> str,
    // Certificates issued; contacts keep them.
    certs: Set[str],
    // Some guardian accepted X, and some accepted Y, at some point.
    sawX: bool,
    sawY: bool,
  }

  var s: State

  action init = s' = {
    promised: GUARDIANS.mapBy(_ => 0),
    acceptedBallot: GUARDIANS.mapBy(_ => 0),
    acceptedValue: GUARDIANS.mapBy(_ => ""),
    reports: 1.to(MAX_BALLOT).mapBy(_ => Map()),
    proposed: Map(),
    certs: Set(),
    sawX: false,
    sawY: false,
  }

  pure def voted(st: State, g: str, b: int, v: str): State = {
    ...st,
    promised: st.promised.set(g, b),
    acceptedBallot: st.acceptedBallot.set(g, b),
    acceptedValue: st.acceptedValue.set(g, v),
    sawX: st.sawX or v == "X",
    sawY: st.sawY or v == "Y",
  }

  // ── Sign once: a guardian approves the first value it is shown. ──
  action signOnce(g: str, v: str): bool = all {
    not(BALLOTS),
    s.acceptedValue.get(g) == "",
    s' = voted(s, g, 1, v),
  }

  // ── Ballots ──
  action promise(g: str, b: int): bool = all {
    BALLOTS,
    b > s.promised.get(g),
    s' = {
      ...s,
      promised: s.promised.set(g, b),
      reports: s.reports.set(b, s.reports.get(b).put(g, (s.acceptedBallot.get(g), s.acceptedValue.get(g)))),
    },
  }

  // The proposer of ballot b, holding promises from quorum q, proposes the
  // value of the highest ballot they report, or any value if none.
  pure def mayPropose(st: State, b: int, q: Set[str], v: str): bool = {
    val reported = q.map(g => st.reports.get(b).get(g)).filter(r => r._1 > 0)
    val highest = reported.fold(0, (m, r) => if (r._1 > m) r._1 else m)
    and {
      not(st.proposed.keys().contains(b)),
      isQuorum(q),
      q.forall(g => st.reports.get(b).keys().contains(g)),
      highest == 0 or reported.exists(r => r._1 == highest and r._2 == v),
    }
  }

  action propose(b: int, q: Set[str], v: str): bool = all {
    BALLOTS,
    mayPropose(s, b, q, v),
    s' = { ...s, proposed: s.proposed.put(b, v) },
  }

  action accept(g: str, b: int): bool = all {
    BALLOTS,
    s.proposed.keys().contains(b),
    b >= s.promised.get(g),
    s' = voted(s, g, b, s.proposed.get(b)),
  }

  // A quorum holding the same (ballot, value) right now makes a certificate.
  action certify(q: Set[str], b: int, v: str): bool = all {
    v != "",
    isQuorum(q),
    q.forall(g => s.acceptedBallot.get(g) == b and s.acceptedValue.get(g) == v),
    s' = { ...s, certs: s.certs.union(Set(v)) },
  }

  // An approval device's state restored from before it voted.
  action rollBack(g: str): bool = all {
    ROLLBACK,
    FAULTY.contains(g),
    s' = {
      ...s,
      promised: s.promised.set(g, 0),
      acceptedBallot: s.acceptedBallot.set(g, 0),
      acceptedValue: s.acceptedValue.set(g, ""),
    },
  }

  action step = {
    nondet g = GUARDIANS.oneOf()
    nondet v = VALUES.oneOf()
    nondet b = 1.to(MAX_BALLOT).oneOf()
    nondet q = QUORUMS.oneOf()
    any {
      signOnce(g, v),
      promise(g, b),
      propose(b, q, v),
      accept(g, b),
      certify(q, b, v),
      rollBack(g),
    }
  }

  // P11: at most one successor is ever certified.
  val one_successor = s.certs.size() <= 1

  // Under sign once, every guardian has signed and no value can still
  // gather a quorum: recovery is stuck for good.
  val can_still_certify =
    BALLOTS or s.certs.size() > 0 or VALUES.exists(v =>
      isQuorum(GUARDIANS.filter(g => s.acceptedValue.get(g) == v or s.acceptedValue.get(g) == ""))
    )

  // A witness, expected violated: after both values were accepted by
  // someone, the ballots still certify one.
  val no_decision_after_competition = not(s.sawX and s.sawY and s.certs.size() == 1)

  // ── Witness traces (`quint test --main=<instance> --match=<prefix>`) ──

  // naive: A1, B1, C1 sign X and A2, B2, C2 sign Y; both are one per group.
  run naive_disjoint_quorums_fork =
    init
      .then(signOnce("A1", "X")).then(signOnce("B1", "X")).then(signOnce("C1", "X"))
      .then(signOnce("A2", "Y")).then(signOnce("B2", "Y")).then(signOnce("C2", "Y"))
      .then(certify(Set("A1", "B1", "C1"), 1, "X"))
      .then(certify(Set("A2", "B2", "C2"), 1, "Y"))
      .expect(s.certs == Set("X", "Y"))

  // sign_once: a three-three split leaves neither value a quorum, for good.
  run sign_once_split_stalls =
    init
      .then(signOnce("A1", "X")).then(signOnce("B1", "X")).then(signOnce("C1", "X"))
      .then(signOnce("A2", "Y")).then(signOnce("B2", "Y")).then(signOnce("C2", "Y"))
      .expect(not(can_still_certify))

  // Ballot 1 certifies X with A1, B1, C1; ballot 2's promises come from
  // A2, B2, C1, and C1 still reports X.
  run ballots_first_round_certifies_x =
    init
      .then(promise("A1", 1)).then(promise("B1", 1)).then(promise("C1", 1))
      .then(propose(1, Set("A1", "B1", "C1"), "X"))
      .then(accept("A1", 1)).then(accept("B1", 1)).then(accept("C1", 1))
      .then(certify(Set("A1", "B1", "C1"), 1, "X"))
      .expect(s.certs == Set("X"))

  // ballots: the overlap guardian C1 reports X, so ballot 2 cannot propose Y.
  run ballots_retry_must_carry_the_certified_value =
    ballots_first_round_certifies_x
      .then(promise("A2", 2)).then(promise("B2", 2)).then(promise("C1", 2))
      .then(propose(2, Set("A2", "B2", "C1"), "Y"))
      .fail()

  // ballots_rolled_back: C1's approval state is restored from before its
  // vote, it reports nothing, and ballot 2 certifies Y as well.
  run rolled_back_overlap_forks =
    ballots_first_round_certifies_x
      .then(rollBack("C1"))
      .then(promise("A2", 2)).then(promise("B2", 2)).then(promise("C1", 2))
      .then(propose(2, Set("A2", "B2", "C1"), "Y"))
      .then(accept("A2", 2)).then(accept("B2", 2)).then(accept("C1", 2))
      .then(certify(Set("A2", "B2", "C1"), 2, "Y"))
      .expect(s.certs == Set("X", "Y"))

  // tolerant_rolled_back: X is certified by four; C1 rolls back, and every
  // four-guardian promise set still holds a guardian reporting X.
  run tolerant_rolled_back_overlap_still_carries_x =
    init
      .then(promise("A1", 1)).then(promise("A2", 1)).then(promise("B1", 1)).then(promise("C1", 1))
      .then(propose(1, Set("A1", "A2", "B1", "C1"), "X"))
      .then(accept("A1", 1)).then(accept("A2", 1)).then(accept("B1", 1)).then(accept("C1", 1))
      .then(certify(Set("A1", "A2", "B1", "C1"), 1, "X"))
      .then(rollBack("C1"))
      .then(promise("A2", 2)).then(promise("B1", 2)).then(promise("B2", 2)).then(promise("C1", 2))
      .then(propose(2, Set("A2", "B1", "B2", "C1"), "Y"))
      .fail()

  // ballots: X reached only A1 before its round was interrupted; ballot 2
  // decides Y, and ballot 1 can no longer finish.
  run ballots_interrupted_round_is_retried_safely =
    init
      .then(promise("A1", 1)).then(promise("B1", 1)).then(promise("C1", 1))
      .then(propose(1, Set("A1", "B1", "C1"), "X"))
      .then(accept("A1", 1))
      .then(promise("A2", 2)).then(promise("B2", 2)).then(promise("C1", 2))
      .then(propose(2, Set("A2", "B2", "C1"), "Y"))
      .then(accept("A2", 2)).then(accept("B2", 2)).then(accept("C1", 2))
      .then(certify(Set("A2", "B2", "C1"), 2, "Y"))
      .expect(s.certs == Set("Y") and s.sawX and s.sawY)
      .then(accept("C1", 1))
      .fail()
}

// One guardian per group, sign once: two disjoint quorums certify X and Y.
module naive {
  import guardian_succession(
    ONE_PER_GROUP = true, BALLOTS = false, ROLLBACK = false, F = 0, FAULTY = Set(),
    GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C", "C2" -> "C")
  ).*
}

// Intersecting quorums, sign once: safe, but split votes stall forever.
module sign_once {
  import guardian_succession(
    ONE_PER_GROUP = false, BALLOTS = false, ROLLBACK = false, F = 0, FAULTY = Set(),
    GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C", "C2" -> "C")
  ).*
}

// Intersecting quorums with ballots: safe, and a retry can still decide.
module ballots {
  import guardian_succession(
    ONE_PER_GROUP = false, BALLOTS = true, ROLLBACK = false, F = 0, FAULTY = Set(),
    GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  ).*
}

// Ballots with P1's bare majority, and C1's approval state can be restored
// from before its votes: two successors again.
module ballots_rolled_back {
  import guardian_succession(
    ONE_PER_GROUP = false, BALLOTS = true, ROLLBACK = true, F = 0, FAULTY = Set("C1"),
    GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  ).*
}

// P19: one faulty approval device tolerated (2q > n + 1, so four of five).
// The same restored device in the overlap no longer forks.
module tolerant_rolled_back {
  import guardian_succession(
    ONE_PER_GROUP = false, BALLOTS = true, ROLLBACK = true, F = 1, FAULTY = Set("C1"),
    GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  ).*
}
