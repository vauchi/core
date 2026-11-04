// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// An inductive proof that one successor is chosen (vauchi/private#796,
// design 2026-11-03-pinned-guardian-policy, P11, P19), at the level of
// Lamport's Voting abstraction of Paxos: each policy guardian's approval
// device keeps the highest ballot it promised (`maxBal`) and its votes;
// a guardian votes for v at b only when v is safe at b, which a proposer
// establishes from a quorum's promises (guardian_succession.qnt models
// those messages). A value is chosen when a quorum voted for it at one
// ballot; once given, a vote counts for ever (`cast`, what certificates
// hold).
//
// A FAULTY device forgets: a restored or cloned approval device resets
// its memory and may vote again, so it can vote both ways. P19's quorum
// (2q > n + F, one per group) makes every two quorums share F + 1
// guardians, so the overlap holds an honest one; safety is stated over
// what an honest guardian's memory shows.
//
// Inductive invariant `inv`: checked to hold initially and to be kept by
// every step from any state satisfying it, and to imply `one_value_chosen`.
// That covers runs of every length for the constants below (five
// guardians, ballots 0 to 2, values X and Y); it is not a proof for other
// sizes.
//
// Checked (Quint 0.33.0, Apalache through `quint verify`, Java 21;
// 2026-11-04), `quint verify --main=<m> --inductive-invariant=inv
// --invariant=one_value_chosen`, also in ci/formal-verification.sh:
//   crash_only                     initial, step and implication hold (78 s)
//   tolerant (P19, C1 forgets)     initial, step and implication hold (87 s)
//   majority_with_a_faulty_device  inv is kept but does not imply
//                                  one_value_chosen: two quorums can share
//                                  only the forgetful device. Reachability of
//                                  that fork is guardian_succession.qnt's
//                                  rolled_back_overlap_forks trace.
// CC-27: without `showsSafeAt` in voteFor, `tolerant`'s step check fails.
module guardian_voting {
  // Equivocating approval devices tolerated by the quorum (P19: 1).
  const F: int
  // Devices that may forget their votes; at most F of them.
  const FAULTY: Set[str]

  pure val GROUP_OF = Map("A1" -> "A", "A2" -> "A", "B1" -> "B", "B2" -> "B", "C1" -> "C")
  pure val GUARDIANS = GROUP_OF.keys()
  pure val GROUPS = GUARDIANS.map(g => GROUP_OF.get(g))
  pure val HONEST = GUARDIANS.exclude(FAULTY)
  pure val VALUES = Set("X", "Y")
  pure val BALLOTS = 0.to(2)

  pure def isQuorum(q: Set[str]): bool = and {
    GROUPS.forall(gr => q.exists(g => GROUP_OF.get(g) == gr)),
    2 * q.size() > GUARDIANS.size() + F,
  }
  pure val QUORUMS = GUARDIANS.powerset().filter(q => isQuorum(q))

  // Device memory, and every vote ever cast.
  var maxBal: str -> int
  var votes: str -> Set[(int, str)]
  var cast: Set[(str, int, str)]

  pure def votedAt(vs: str -> Set[(int, str)], a: str, b: int): bool =
    vs.get(a).exists(vt => vt._1 == b)

  // What a proposer can show from a quorum's promises: the quorum promised
  // b, and either none of it voted below b, or the highest ballot c below b
  // at which any of it voted had v, with nobody of it voting between c and
  // b. Only honest members' memory is consulted: a faulty one's may have
  // been wiped.
  def showsSafeAt(b: int, v: str): bool =
    QUORUMS.exists(q => {
      val h = q.intersect(HONEST)
      and {
        h.forall(a => maxBal.get(a) >= b),
        BALLOTS.union(Set(-1)).filter(c => c < b).exists(c => and {
          c == -1 or h.exists(a => votes.get(a).contains((c, v))),
          BALLOTS.filter(d => d > c and d < b).forall(d => h.forall(a => not(votedAt(votes, a, d)))),
        }),
      }
    })

  action init = all {
    maxBal' = GUARDIANS.mapBy(_ => -1),
    votes' = GUARDIANS.mapBy(_ => Set()),
    cast' = Set(),
  }

  action increaseMaxBal(a: str, b: int): bool = all {
    b > maxBal.get(a),
    maxBal' = maxBal.set(a, b),
    votes' = votes,
    cast' = cast,
  }

  // One proposer per ballot proposes one value: no two votes at b differ.
  action voteFor(a: str, b: int, v: str): bool = all {
    maxBal.get(a) <= b,
    not(votedAt(votes, a, b)),
    cast.forall(vt => vt._2 == b implies vt._3 == v),
    showsSafeAt(b, v),
    maxBal' = maxBal.set(a, b),
    votes' = votes.setBy(a, vs => vs.union(Set((b, v)))),
    cast' = cast.union(Set((a, b, v))),
  }

  // A restored or cloned approval device: its memory starts afresh.
  action forget(a: str): bool = all {
    FAULTY.contains(a),
    maxBal' = maxBal.set(a, -1),
    votes' = votes.set(a, Set()),
    cast' = cast,
  }

  action step = {
    nondet a = GUARDIANS.oneOf()
    nondet b = BALLOTS.oneOf()
    nondet v = VALUES.oneOf()
    any { increaseMaxBal(a, b), voteFor(a, b, v), forget(a) }
  }

  def chosenAt(b: int, v: str): bool =
    QUORUMS.exists(q => q.forall(a => cast.contains((a, b, v))))
  def chosen: Set[str] = VALUES.filter(v => BALLOTS.exists(b => chosenAt(b, v)))

  // P11: at most one value is ever chosen.
  val one_value_chosen = chosen.size() <= 1

  // ── The inductive invariant ──
  // Typing first: from it the induction step draws an arbitrary state.
  val type_ok = and {
    maxBal.in(setOfMaps(GUARDIANS, (-1).to(2))),
    votes.in(setOfMaps(GUARDIANS, tuples(BALLOTS, VALUES).powerset())),
    cast.in(tuples(GUARDIANS, BALLOTS, VALUES).powerset()),
  }
  // An honest device remembers every vote it cast, at ballots up to maxBal.
  val honest_memory = HONEST.forall(a => and {
    votes.get(a) == cast.filter(vt => vt._1 == a).map(vt => (vt._2, vt._3)),
    votes.get(a).forall(vt => vt._1 <= maxBal.get(a)),
  })
  // A faulty device remembers only votes it cast.
  val faulty_memory = FAULTY.forall(a =>
    votes.get(a).forall(vt => cast.contains((a, vt._1, vt._2)) and vt._1 <= maxBal.get(a)))
  val one_value_per_ballot =
    cast.forall(x => cast.forall(y => x._2 == y._2 implies x._3 == y._3))
  // v is safe at b (Lamport's SafeAt, over honest members): at every ballot
  // c below b, some quorum's honest members each voted v at c or can no
  // longer vote at c. Unlike what a proposer shows, this stays true.
  def cannotVoteAt(a: str, c: int): bool = maxBal.get(a) > c and not(votedAt(votes, a, c))
  def safeAt(b: int, v: str): bool =
    BALLOTS.filter(c => c < b).forall(c => QUORUMS.exists(q =>
      q.intersect(HONEST).forall(a => votes.get(a).contains((c, v)) or cannotVoteAt(a, c))))
  // Every vote cast is safe.
  val votes_safe = cast.forall(vt => safeAt(vt._2, vt._3))

  val inv = and { type_ok, honest_memory, faulty_memory, one_value_per_ballot, votes_safe }
}

// P19: one faulty device, quorum 2q > n + 1 (four of five, one per group).
module tolerant {
  import guardian_voting(F = 1, FAULTY = Set("C1")).*
}

// No faulty device, P1's bare majority: the crash-fault baseline.
module crash_only {
  import guardian_voting(F = 0, FAULTY = Set()).*
}

// P1's bare majority with one faulty device: expected to fail, the
// negative control (the invariant is not inductive and the safety
// property is violated).
module majority_with_a_faulty_device {
  import guardian_voting(F = 0, FAULTY = Set("C1")).*
}
