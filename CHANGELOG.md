<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Changelog

All notable changes to vauchi-core are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

### Changed

- `AppEngine::can_go_back()` now gates on `AppScreen::is_root()` in
  addition to `nav_history` emptiness. Roots (Onboarding + the five
  mobile bottom-nav tabs: MyInfo, Contacts, Exchange, Groups, More) are
  back-stoppers — back at a root must exit, not pop history. Fixes the
  post-onboarding gotcha where onboarding crumbs in `nav_history`
  produced a phantom back affordance at the home tab. `navigate_back`
  itself is unchanged, so programmatic back-after-tab-switch behavior
  is preserved. Resolves the "decide `can_go_back` semantics" item in
  the CoreScreenIdMap rework plan.

### Added

- `AppScreen::is_root()` — declares which screens are navigation roots
  (closed set on the enum). Single source of truth for "back exits vs
  back pops" at the screen level, so frontends never carry a tab-root
  list.

## [0.51.18] — 2026-05-28

### Added

- `PlatformAppEngine::can_go_back()` — UniFFI query returning whether
  core's `nav_history` holds a back step. Frontends drive their back
  affordance / `BackHandler` from this instead of inferring it from a
  frontend-side screen-id map. `HUMBLE_ALLOWLIST` 25 → 26 (ADR-043
  Amendment 4 legitimate: a query, not domain logic). Tier-0 item 1 of
  the CoreScreenIdMap rework
  (`_private/docs/planning/todo/2026-05-27-corescreenidmap-rework-plan.md`).
- Reserved global-chrome action id `"open_settings"`: the native
  top-bar gear forwards
  `UserAction::ActionPressed { action_id: "open_settings" }` instead of
  constructing the "Settings" screen name. Core intercepts it before
  per-screen dispatch and resolves to `NavigateTo(Settings)`, on the
  same closed-set basis as `"open_update_link"`. Tier-0 item 2 of the
  same rework.

### Internal

- Retired the dead device-link relay surface
  (`vauchi-app/orchestrator/device_link_relay.rs` 325 → 84 lines): the
  5 superseded single-call fns (`listen_for_request`,
  `create_offer_and_listen`, `poll_for_claim`, `send_and_receive`,
  `poll_for_response`) and their orphan supporting code (`create_offer`,
  `claim_and_send_request`, `send_response`, `DeviceLinkRelayMessage`,
  `DeviceLinkError`, test-only encode/decode helpers). Surviving surface
  = `DeviceLinkBroker` trait + `ClaimPayload` — the live broker
  abstraction the state machines drive (slice 32l Phase 1). Zeroed 4 of
  6 residual `core_instant_now` ratchet sites. The deprecated
  `VauchiPlatform::*` device-link methods listed in §[0.24.1] remain;
  their retirement is the separate slice 32l T3.1b work, blocked on the
  windows `VauchiNative.cs` C# consumer migration.

## [0.25.0] — 2026-04-26

### Added

- `recovery_public_key_hex_length()` and `recovery_claim_min_input_length()`
  UniFFI free functions returning `64` and `20` respectively. Frontends
  (iOS `RecoveryView`, Android `RecoveryScreen`) source these instead
  of hardcoding the magic numbers, closing the recovery-flow tail of
  §1B in
  `_private/docs/problems/2026-04-16-frontend-pure-renderer-violations/`.
  The constants live as `pub const RECOVERY_PUBLIC_KEY_HEX_LEN` /
  `RECOVERY_CLAIM_MIN_INPUT_LEN` in `vauchi-core::recovery` and are now
  also consumed by `vauchi-app::ui::recovery_help` (replacing two
  inline literal `>= 20` checks) so the rule has one source of truth
  across core and frontends.

## [0.24.1] — 2026-04-26

### Added

- `MobileDeviceLinkSession` + `DeviceLinkSessionListener` (UniFFI) —
  Phase 1 of the device-link orchestrator
  (`_private/docs/problems/2026-04-25-device-link-orchestrator/`).
  Single core-owned session handle replaces the four per-frontend
  device-link state machines. Cycle thread drives QR-ready emit →
  relay listen → confirmation prompt → user-action wait via
  `mpsc::sync_channel(1)` (capacity 1 = double-tap idempotent) →
  `confirm_link` → `save_device_registry` → response send → terminal
  callbacks. Mirrors G4 Phase 2.5 (`MultiStageSessionListener`,
  `core!668`). Initiator-only Phase 1 — responder-side reserved for a
  follow-up record. The session also closes a pre-existing gap: the
  legacy `MobileDeviceLinkInitiator::confirm_link_with_proof`
  discarded the updated `DeviceRegistry`; the orchestrator persists
  it before posting the response.
- `VauchiPlatform::create_device_link_session_initiator()` —
  production factory for the new session.

### Internal

- `device_link_relay` split into `create_offer` + `poll_for_claim`
  and `claim_and_send_request` + `poll_for_response`. Legacy
  `create_offer_and_listen` / `send_and_receive` become 3-line shims
  with a never-tripped cancel flag. Lets the orchestrator own the
  deadline math (`qr_timestamp + LINK_QR_EXPIRY_SECONDS`) and observe
  cancel on the existing 1 s poll cadence.

### Deprecated

The seven legacy device-link UniFFI items (4 `VauchiPlatform`
methods + 2 wrapper structs) are marked `#[deprecated]`. Frontends
have one binding-republish cycle to migrate before Phase 3 deletes
them:

- `VauchiPlatform::start_device_link`
- `VauchiPlatform::listen_for_device_link_request`
- `VauchiPlatform::send_device_link_response`
- `MobileDeviceLinkInitiator` (struct)
- `VauchiPlatform::start_device_join` (responder, reserved for the
  deferred responder orchestrator)
- `VauchiPlatform::send_device_link_request` (responder, reserved)
- `MobileDeviceLinkResponder` (struct, reserved)

## [0.24.0] — 2026-04-26

### Added

- `VauchiPlatform::contact_detail_footer_action_id(contact_id)` —
  returns `"delete_contact"` (imported) or `"archive_contact"`
  (exchanged), the footer-button id `ContactDetailEngine` would emit.
  Frontends dispatch on the returned id so the view layer stops
  branching on `MobileContact.is_imported` directly. Closes the
  iOS/Android tail of §1A pure-renderer cleanup —
  `_private/docs/problems/2026-04-25-isimported-frontend-cleanup/`.
  Helper also exposed as `vauchi_app::ui::contact_detail_footer_action_id`
  for desktop frontends.
- `mobile_is_valid_pem_certificate(value)` — UniFFI free function
  that returns `true` if the trimmed input begins with
  `-----BEGIN CERTIFICATE-----` and ends with
  `-----END CERTIFICATE-----`. Replaces the per-frontend
  `isValidPem` regex on iOS `SettingsView`. Other PEM labels
  (`PRIVATE KEY`, …) are rejected so the consumer can render a
  "this is not a certificate" hint. Real cryptographic validation
  still happens in the rustls verifier when the cert is consumed by
  `set_pinned_certificate`.

## [0.23.0] — 2026-04-25

### Removed

- Deprecated polling getters on `MobileMultiStageSession`:
  `get_display_qr`, `get_state`, `get_received_data`,
  `get_transport_key`. Use the `MultiStageSessionListener` callbacks
  introduced in 0.22.0 instead. G4 Phase 3 dead-code removal.
- `VauchiPlatform::finalize_multistage_exchange` — listener-path
  persistence (Phase 2.5) makes the explicit finalize call
  unnecessary; the cycle thread persists the contact + ratchet state
  before firing `on_finalized`.

### Fixed

- Listener-path contact persistence regression: cycle thread now
  captures `received_data` + `transport_key` at the Finalized
  transition and runs the `Contact::from_exchange` →
  `save_contact` → `DoubleRatchetState::initialize_initiator` →
  `save_ratchet_state` body before `on_finalized` fires. On
  persistence failure emits `on_state_changed(Failed{reason})` and
  skips `on_finalized`. G4 Phase 2.5.

## [0.22.0] — 2026-04-24

### Added

- `MultiStageSessionListener` UniFFI callback interface plus
  `MobileMultiStageSession::set_listener` / `start` / `cancel` lifecycle —
  core now owns the multi-stage exchange protocol clock via a
  `vauchi-exchange-cycle` thread. Frontends drop their `Timer` /
  `LaunchedEffect` polling loops and render events (`on_qr_payload`,
  `on_state_changed`, `on_finalized(contact_name)`, `on_session_ended`)
  as they arrive. G4 Phase 1 — see
  `_private/docs/problems/2026-04-23-g4-exchange-event-api/`.

### Deprecated

- `MobileMultiStageSession::get_display_qr`, `get_state`,
  `get_received_data`, `get_transport_key` — use the listener callbacks
  instead. Retained through 0.22.x so iOS + Android can migrate in
  sequence; removed in 0.23.

## [0.11.1] — 2026-03-29

### Fixed

- COMBO QR error correction test guard (multistage_e2e_tests)

### Changed

- Minimum Rust version set to 1.93 in workspace manifest
- Added rstest dependency for parameterized social URI tests

## [0.11.0] — 2026-03-28

### Added

- Encrypted exchange APIs for ADR-021 compliance
  (`accept_relay_exchange`, `accept_encrypted_relay_exchange`)
- Hide/unhide contact toggle in ContactDetail screen
- Fingerprint verification engine with verify/unverify API
- Encrypted personal notes (`add_personal_note`, `read_personal_note`)
- Persistent sent delta version tracking (migration v36)
- `prepare_card_update_for_contact()` API for targeted card updates
- `#[non_exhaustive]` on all public enums (future semver safety)
- CABI Windows build + cosign distribution pipeline

### Changed

- COMBO QR error correction raised from M (15%) to Q (25%) for
  better iPhone scan reliability
- Renamed "account" terminology to "identity" across all APIs
  (`scheduleAccountDeletion` → `scheduleIdentityDeletion`, etc.)
- Removed community scoring / field validation APIs (unused)
- Unified card propagation through single crypto path
- Minimum Rust version: 1.93

### Fixed

- Fingerprint verification now clears `has_recovered` flag
- Trust level enforcement for recovery trust assignment
- Blocked contacts rejected in `prepare_card_update_for_contact`
- TOCTOU double lookup eliminated in fingerprint verify screen
- Contact visibility changes persisted on save
- All adversarial-reachable `.unwrap()` calls eliminated

## [0.10.6] — 2026-03-25

Initial versioned release with platform bindings for iOS, macOS,
and Android via vauchi-platform-swift and Maven AAR.
