// SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me>
//
// SPDX-License-Identifier: AGPL-3.0-or-later

// The BLE exchange screen (`ui/exchange/ble_engine.rs`, the chrome) and the
// AppEngine-owned handshake machine (`orchestrator/ble_handshake_machine.rs`)
// driven together (vauchi/private#842). Crypto is left out
// (ble_handshake.spthy covers it); this covers what the pair can end up in.
//
// One tick is 10 s: the chrome fails a step after BLE_STEP_TIMEOUT_SECS =
// 60 s with no progress (`tick` → `force_failure`). The machine has no
// deadline of its own; before #843 `sync_ble_handshake_lifecycle` cancelled
// it only when the screen changed, and `start_ble_handshake_on_discovery`
// is a no-op while a session is held. A peer's writes reach the machine
// over the open link whatever the chrome shows, and `apply_ble_machine_event`
// persists the contact on `Completed`.
//
// Run:
//   quint run --main=before_843 --invariant=no_live_machine_behind_a_failed_screen
//   quint run --main=before_843 --invariant=a_retry_starts_from_no_machine
//   quint run --main=before_843 --invariant=a_contact_is_persisted_only_on_a_live_screen
//   quint verify --main=failure_cancels_the_machine --invariant=...

module ble_exchange_machine {
  // On since #843 (core!2031): a step failure and a Retry tear the machine
  // down. Off models the code before it.
  const CANCEL_ON_FAILURE: bool

  pure val STEP_TIMEOUT = 6
  pure val MAX_T = 24

  type Screen = Discovering | Handshaking | Exchanging | Verifying | Success | Failed
  type Machine = NoMachine | MPreparing | MHandshaking | MTransferring | MVerifying
               | MCompleted | MFailed | MCancelled

  type State = {
    now: int,
    screen: Screen,
    stepEntered: int,
    machine: Machine,
    linked: bool,
    contactPersisted: bool,
    retried: bool,
  }

  var s: State

  pure def screenActive(sc: Screen): bool =
    sc == Discovering or sc == Handshaking or sc == Exchanging or sc == Verifying
  pure def machineLive(m: Machine): bool =
    m == MHandshaking or m == MTransferring or m == MVerifying
  pure def machineTerminal(m: Machine): bool =
    m == MCompleted or m == MFailed or m == MCancelled

  action init = s' = {
    now: 0,
    screen: Discovering,
    stepEntered: 0,
    machine: NoMachine,
    linked: false,
    contactPersisted: false,
    retried: false,
  }

  // BleExchangeEngine::tick — the stall deadline fails the chrome only.
  action tick = all {
    s.now < MAX_T,
    s' = if (screenActive(s.screen) and s.now + 1 - s.stepEntered >= STEP_TIMEOUT)
           { ...s, now: s.now + 1, screen: Failed,
             machine: if (CANCEL_ON_FAILURE and not(machineTerminal(s.machine))) MCancelled
                      else s.machine }
         else { ...s, now: s.now + 1 },
  }

  // A discovery builds the session — only while none is held.
  action discover = all {
    s.screen == Discovering,
    s.machine == NoMachine,
    s' = { ...s, machine: MPreparing },
  }

  // BleConnected: the machine and the chrome both advance.
  action connect = all {
    s.machine == MPreparing,
    s' = { ...s, machine: MHandshaking, linked: true,
           screen: if (s.screen == Discovering) Handshaking else s.screen,
           stepEntered: if (s.screen == Discovering) s.now else s.stepEntered },
  }

  // The peer's writes drive the machine over the open link, whatever the
  // chrome shows; the chrome follows only while it is still active.
  action peerProgress = all {
    machineLive(s.machine),
    s.linked,
    s' = match s.machine {
      | MHandshaking => { ...s, machine: MTransferring,
          screen: if (screenActive(s.screen)) Exchanging else s.screen,
          stepEntered: if (screenActive(s.screen)) s.now else s.stepEntered }
      | MTransferring => { ...s, machine: MVerifying,
          screen: if (screenActive(s.screen)) Verifying else s.screen,
          stepEntered: if (screenActive(s.screen)) s.now else s.stepEntered }
      | MVerifying => { ...s, machine: MCompleted, contactPersisted: true,
          screen: if (screenActive(s.screen)) Success else s.screen }
      | _ => s
    },
  }

  // BleDisconnected on the active link.
  action disconnect = all {
    machineLive(s.machine),
    s.linked,
    s' = { ...s, machine: MFailed, linked: false,
           screen: if (screenActive(s.screen)) Failed else s.screen },
  }

  // The user taps Retry on the failed chrome.
  action retry = all {
    s.screen == Failed,
    s' = { ...s, screen: Discovering, stepEntered: s.now, retried: true,
           machine: if (CANCEL_ON_FAILURE) NoMachine else s.machine },
  }

  action step = any {
    tick,
    discover,
    connect,
    peerProgress,
    disconnect,
    retry,
  }

  // A failed chrome has no machine still talking to the peer behind it.
  val no_live_machine_behind_a_failed_screen: bool =
    s.screen == Failed implies not(machineLive(s.machine))

  // After Retry the discovering chrome is not stuck behind a finished
  // session: `start_ble_handshake_on_discovery` builds nothing while one is
  // held.
  val a_retry_starts_from_no_machine: bool =
    (s.retried and s.screen == Discovering) implies not(machineTerminal(s.machine))

  // A contact lands only while the chrome can show the success.
  val a_contact_is_persisted_only_on_a_live_screen: bool =
    s.contactPersisted implies s.screen == Success
}

// Before #843: the stall deadline failed the chrome and left the machine
// holding the link. Violates all three invariants.
module before_843 {
  import ble_exchange_machine(CANCEL_ON_FAILURE = false).*
}

// What main runs since #843: the chrome's failure and Retry cancel the
// machine.
module failure_cancels_the_machine {
  import ble_exchange_machine(CANCEL_ON_FAILURE = true).*
}
