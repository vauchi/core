// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// The device-link initiator (host) and responder (joiner) machines in
// vauchi-app/src/orchestrator/ (#757), driven against each other over the
// relay's rendezvous slots. Crypto is left out (device_link.spthy covers
// it); this covers what the two machines can end up in.
//
// One tick is 60 s: the QR lives LINK_QR_EXPIRY_SECONDS = 300 s and counts
// as expired only after that (`now > expires_at`); both machines use
// QR_WINDOW_SECS = 300 s as relay budget and slot lifetime; the host gives
// the user USER_CONFIRM_TIMEOUT_S = 60 s once a claim arrives. Deadlines
// fire at `now >= deadline`, as in `advance`.
//
// Run:
//   quint run --main=reliable_relay --invariant=no_device_registered_for_a_failed_joiner
//   quint run --main=reliable_relay --invariant=joiner_seed_implies_registered
module device_link_machines {
  const RELAY_RELIABLE: bool
  const JOINER_MAY_CANCEL: bool
  const FINALIZE_CHECKS_QR_EXPIRY: bool

  pure val QR_EXPIRES_AT = 5
  pure val RELAY_TIMEOUT = 5
  pure val CONFIRM_TIMEOUT = 1
  pure val MAX_T = 9

  type Host = HPreparing | HAwaitingClaim | HAwaitingConfirmation | HFinalizing
            | HCompleted | HFailed | HCancelled
  type Joiner = JIdle | JScanning | JAwaitingResponse | JCompleted | JFailed | JCancelled
  type Reply = NoReply | Seed | Refusal

  type State = {
    now: int,
    host: Host,
    hostDeadline: int,
    confirmDeadline: int,
    joiner: Joiner,
    joinerDeadline: int,
    claimPosted: bool,
    returnSlotExpiry: int,
    reply: Reply,
    deviceRegistered: bool,
  }

  var s: State

  pure def min(a: int, b: int): int = if (a < b) a else b

  pure def hostTerminal(h: Host): bool = h == HCompleted or h == HFailed or h == HCancelled

  pure def joinerTerminal(j: Joiner): bool = j == JCompleted or j == JFailed or j == JCancelled

  // `refuse`: best effort, so a lost refusal leaves the joiner to its deadline.
  pure def refuse(st: State, ok: bool, outcome: Host): State =
    { ...st, host: outcome, reply: if (ok and st.now < st.returnSlotExpiry) Refusal else st.reply }

  val relayOutcomes: Set[bool] = if (RELAY_RELIABLE) Set(true) else Set(true, false)

  action init = s' = {
    now: 0,
    host: HPreparing,
    hostDeadline: 0,
    confirmDeadline: 0,
    joiner: JIdle,
    joinerDeadline: 0,
    claimPosted: false,
    returnSlotExpiry: 0,
    reply: NoReply,
    deviceRegistered: false,
  }

  action tick = all {
    s.now < MAX_T,
    s' = { ...s, now: s.now + 1 },
  }

  // DeviceLinkInitiatorMachine::advance
  action hostAdvance = {
    nondet ok = relayOutcomes.oneOf()
    all {
      not(hostTerminal(s.host)),
      s' = match s.host {
        | HPreparing =>
            if (ok) { ...s, host: HAwaitingClaim, hostDeadline: s.now + RELAY_TIMEOUT }
            else { ...s, host: HFailed }
        | HAwaitingClaim =>
            if (s.now >= s.hostDeadline) { ...s, host: HFailed }
            else if (not(ok)) { ...s, host: HFailed }
            else if (s.claimPosted)
              { ...s, host: HAwaitingConfirmation, confirmDeadline: s.now + CONFIRM_TIMEOUT }
            else s
        | HAwaitingConfirmation =>
            if (s.now >= s.confirmDeadline) refuse(s, ok, HFailed) else s
        // Fails at the QR's expiry, the joiner's own deadline (#808);
        // `finalize` persists the registry before it sends the response.
        | HFinalizing =>
            if (FINALIZE_CHECKS_QR_EXPIRY and s.now >= QR_EXPIRES_AT) { ...s, host: HFailed }
            else if (ok and s.now < s.returnSlotExpiry)
              { ...s, deviceRegistered: true, host: HCompleted, reply: Seed }
            else { ...s, deviceRegistered: true, host: HFailed }
        | _ => s
      },
    }
  }

  // confirm_manual does not look at the confirmation deadline.
  action hostConfirm = all {
    s.host == HAwaitingConfirmation,
    s' = { ...s, host: HFinalizing },
  }

  action hostDeny = {
    nondet ok = relayOutcomes.oneOf()
    all {
      s.host == HAwaitingConfirmation,
      s' = refuse(s, ok, HFailed),
    }
  }

  action hostCancel = {
    nondet ok = relayOutcomes.oneOf()
    all {
      not(hostTerminal(s.host)),
      s' = if (s.host == HAwaitingConfirmation) refuse(s, ok, HCancelled)
           else { ...s, host: HCancelled },
    }
  }

  // The invitation exists only while the host is in AwaitingClaim.
  action joinerScan = all {
    s.joiner == JIdle,
    s.host == HAwaitingClaim,
    s' = { ...s, joiner: JScanning },
  }

  // DeviceLinkResponderMachine::advance
  action joinerAdvance = {
    nondet ok = relayOutcomes.oneOf()
    all {
      s.joiner == JScanning or s.joiner == JAwaitingResponse,
      s' = match s.joiner {
        | JScanning =>
            if (s.now > QR_EXPIRES_AT) { ...s, joiner: JFailed }
            // The claim lands only while the host's offer slot lives.
            else if (ok and s.now < s.hostDeadline)
              { ...s, joiner: JAwaitingResponse, claimPosted: true,
                returnSlotExpiry: s.now + RELAY_TIMEOUT,
                joinerDeadline: min(s.now + RELAY_TIMEOUT, QR_EXPIRES_AT) }
            else { ...s, joiner: JFailed }
        // Polls before it looks at the deadline (#808).
        | JAwaitingResponse =>
            if (not(ok)) { ...s, joiner: JFailed }
            else match s.reply {
              | Seed => { ...s, joiner: JCompleted }
              | Refusal => { ...s, joiner: JFailed }
              | NoReply => if (s.now >= s.joinerDeadline) { ...s, joiner: JFailed } else s
            }
        | _ => s
      },
    }
  }

  action joinerCancel = all {
    JOINER_MAY_CANCEL,
    not(joinerTerminal(s.joiner)),
    s' = { ...s, joiner: JCancelled },
  }

  action step = any {
    tick,
    hostAdvance,
    hostConfirm,
    hostDeny,
    hostCancel,
    joinerScan,
    joinerAdvance,
    joinerCancel,
  }

  // @claim: S15
  // The joiner holds the seed only for a device the host registered.
  val joiner_seed_implies_registered: bool =
    s.joiner == JCompleted implies s.deviceRegistered

  // @claim: S15
  // The host never lists a device whose joiner gave up.
  val no_device_registered_for_a_failed_joiner: bool =
    not(s.deviceRegistered and (s.joiner == JFailed or s.joiner == JCancelled))
}

// A relay that delivers everything and a joiner who waits: whatever goes
// wrong here comes from the machines' own timing.
module reliable_relay {
  import device_link_machines(
    RELAY_RELIABLE = true, JOINER_MAY_CANCEL = false, FINALIZE_CHECKS_QR_EXPIRY = true
  ).*
}

// Without the host's expiry check (#808): violates
// no_device_registered_for_a_failed_joiner.
module before_808 {
  import device_link_machines(
    RELAY_RELIABLE = true, JOINER_MAY_CANCEL = false, FINALIZE_CHECKS_QR_EXPIRY = false
  ).*
}

// A lost response or a joiner that cancels after the host confirmed still
// registers a device without its seed: the registry is saved first.
module lossy_relay {
  import device_link_machines(
    RELAY_RELIABLE = false, JOINER_MAY_CANCEL = true, FINALIZE_CHECKS_QR_EXPIRY = true
  ).*
}
