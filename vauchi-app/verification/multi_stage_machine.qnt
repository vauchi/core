// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// The multi-stage exchange machine (`orchestrator/multi_stage_machine.rs`:
// Glance, Hover, Tap-Hover-Shake) and whether it can wait forever
// (vauchi/private#842). Frames, audio and the accelerometer are left out;
// a peer frame either advances the phase or does not.
//
// One tick is 10 s: `advance` fails a phase after
// MULTI_STAGE_STEP_TIMEOUT_MS = 120 s without a transition, the peerless
// Advertising phase after MULTI_STAGE_DISCOVERY_TIMEOUT_MS (longer), and
// Finalized has no deadline because the next `advance` with nothing left
// to display completes it. `advance` runs on every engine tick.
//
// Run (Apalache: a phase outliving its budget needs 13 silent ticks, which
// random simulation never draws):
//   quint verify --max-steps=15 --main=main_today --invariant=no_phase_outlives_its_budget
//   quint verify --max-steps=15 --main=no_step_timeout --invariant=no_phase_outlives_its_budget

module multi_stage_machine {
  // Sentinel: the per-step deadline switched off.
  const STEP_TIMEOUT_ENFORCED: bool

  pure val STEP_TIMEOUT = 12
  pure val DISCOVERY_TIMEOUT = 18
  pure val MAX_T = 60

  type Phase = Preparing | Advertising | Discovered | Transferring | Verifying
             | Confirming | Finalized | Completed | Failed | Cancelled

  type State = { now: int, phase: Phase, phaseEntered: int }

  var s: State

  pure def terminal(p: Phase): bool = p == Completed or p == Failed or p == Cancelled

  pure def budget(p: Phase): int =
    if (p == Advertising) DISCOVERY_TIMEOUT
    else if (p == Finalized) 1
    else STEP_TIMEOUT

  pure def following(p: Phase): Phase = match p {
    | Preparing => Advertising
    | Advertising => Discovered
    | Discovered => Transferring
    | Transferring => Verifying
    | Verifying => Confirming
    | Confirming => Finalized
    | _ => p
  }

  action init = s' = { now: 0, phase: Preparing, phaseEntered: 0 }

  // One engine tick, then `advance`: the deadline, or Finalized's
  // completion once nothing is left to display.
  action tick = all {
    s.now < MAX_T,
    s' = {
      val now1 = s.now + 1
      if (terminal(s.phase)) { ...s, now: now1 }
      else if (s.phase == Finalized) { ...s, now: now1, phase: Completed, phaseEntered: now1 }
      else if (STEP_TIMEOUT_ENFORCED and now1 - s.phaseEntered >= budget(s.phase))
        { ...s, now: now1, phase: Failed, phaseEntered: now1 }
      else { ...s, now: now1 }
    },
  }

  // A peer frame (or our own first frame) moves the phase on.
  action peerFrame = all {
    not(terminal(s.phase)),
    s.phase != Finalized,
    s' = { ...s, phase: following(s.phase), phaseEntered: s.now },
  }

  // HardwareError, PermissionDenied, a missing camera or microphone.
  action hardwareFailure = all {
    not(terminal(s.phase)),
    s' = { ...s, phase: Failed, phaseEntered: s.now },
  }

  action cancel = all {
    not(terminal(s.phase)),
    s' = { ...s, phase: Cancelled, phaseEntered: s.now },
  }

  action step = any { tick, peerFrame, hardwareFailure, cancel }

  // No waiting phase lasts longer than its budget: with seven finite
  // budgets every run ends.
  val no_phase_outlives_its_budget: bool =
    not(terminal(s.phase)) implies s.now - s.phaseEntered <= budget(s.phase)
}

module main_today {
  import multi_stage_machine(STEP_TIMEOUT_ENFORCED = true).*
}

// Without the per-step deadline a phase waits for a peer that never comes.
module no_step_timeout {
  import multi_stage_machine(STEP_TIMEOUT_ENFORCED = false).*
}
