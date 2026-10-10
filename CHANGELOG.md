<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Changelog

All notable changes to vauchi-core are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [0.79.0] — 2026-10-10

### Changed

- A successful biometric prompt on Core's lock screen is acted on by Core
  (vauchi/private#591). Without a duress PIN the app opens on its default
  screen. With one, the lock screen stays, withdraws the biometric action
  and takes the password, which picks normal or duress mode. Every shell,
  macOS included, gets duress without a native app-password screen.
  `SetAuthenticationRequirement` is still sent first in the batch.

## [0.78.2] — 2026-10-10

### Fixed

- The in-app emergency wipe is the crypto-shred (vauchi/private#599): it
  deletes the SMK and the bootstrap key from secure storage, then the
  database and the data directory, so a copy of the data taken before the
  wipe no longer opens. It used to delete rows and keep every key.
- After a shred the engine reopens on a fresh install, on mobile and on the
  desktop C ABI, keeping the render context, capabilities, network state
  and event listener; it used to keep the deleted database (the desktop
  could not create a new identity afterwards).
- Settings → Advanced → Wipe All Data opens the shred screen, where typing
  WIPE runs the shred; its inline confirm deleted nothing
  (vauchi/private#598).
- A keychain callback that fails in a way the platform did not declare
  reaches Core as a keychain failure instead of aborting the process.

### Added

- `Storage::db_path`, `AppEngine::device_capabilities`,
  `AppEngine::mark_storage_shredded` and `AppEngine::take_storage_shredded`.

## [0.78.1] — 2026-10-10

### Fixed

- An upgrade whose keychain takes a write only after the person unlocks
  starts on Core's unlock screen, and the unlock finishes the move to the
  SMK. Storing the SMK used to turn the keychain's answer into a
  configuration error, so the app failed to start (found on a device,
  vauchi/private#580).
- Storing the SMK classifies keychain failures as loading keys does: any
  failure other than a lock or a lost key offers Try again, including a
  desktop keyring that does not answer (ADR-045).

## [0.78.0] — 2026-10-09

### Fixed

- A locked start answers the shell's environment with a presentation
  profile, and the engine it opens after the unlock gets the environment the
  shell reported, so shells that wait for a profile no longer show a spinner
  on a locked start or right after the unlock (vauchi/private#580).
- When the unlock prompt opens a locked start, the first batch carries
  `SetAuthenticationRequirement`: `app_password` when a duress PIN is set
  up, so the app-password screen is no longer skipped (ADR-032,
  vauchi/private#580).
- Rekey works with imported contacts and with contacts whose card is under
  a CEK; either one used to fail the whole rekey, including the boot path
  that finishes the SMK move (vauchi/private#585).
- An imported contact's vCard UID is stored encrypted, with a keyed hash for
  the duplicate-import lookup (migration v80, vauchi/private#579).

### Removed

- `Vauchi::migrate_contacts_to_cek` and
  `Vauchi::migrate_field_centric_visibility`, one-shot upgrade routines for
  installs that predate them; no consumer called them (vauchi/private#572).

## [0.77.0] — 2026-10-09

Crypto-shredding now holds on mobile and desktop (vauchi/private#580 and
vauchi/private#581): every key that opens the database lives in the
platform keychain, and a shred deletes them all.

### Added

- Storage key check (migration v75): storage opens only under the key its
  data is encrypted with; `StorageError::WrongKey` otherwise.
- `PlatformAppEngine::open_with_keychain(data_dir, relay_url,
  shell_storage_key, keychain)` with `HandedOverSecret { handle, secret }`
  (ADR-043 Amendment 7); `Command::ForgetStoredSecret { handle }` tells the
  shell when it may delete a handed-over key.
- Locked start: a keychain that needs authentication, is unavailable, or no
  longer opens the data starts on Core's storage-lock screens (unlock, Try
  again, confirmed Start over) instead of failing construction — on mobile
  and in `vauchi_app_create_with_keyring` (desktop).
- `KeychainError::AuthenticationRequired` / `KeyInvalidated`;
  `StorageError::SecureStorageLocked` / `SecureStorageKeyInvalidated` /
  `SecureStorageUnavailable`.
- `vauchi_core::api::storage_reset::delete_unreadable_data`.

### Changed

- With secure storage, boot keeps a bootstrap key before an identity
  exists, moves the data to the SMK-derived key at identity creation or the
  first boot after an upgrade, and finishes a move a crash interrupted.
- Every shred deletes the bootstrap key with the SMK.

### Fixed

- A shred uses stored pre-signed messages only if the current identity
  signed them (vauchi/private#582).

Entries for 0.52–0.76 were not kept; see the git history.

## [0.51.21] — 2026-05-28

### Added

- `Component::Indicator { id, label, kind, action_id, a11y }` —
  generic ongoing-status primitive for chrome (sync / offline /
  backup / update). Distinct from `Component::StatusIndicator`
  (screen-body in-progress status); Indicator is chrome-positioned.
  `IndicatorKind`: `Active` / `Error` / `Neutral` / `Busy`
  (presentation-shaped semantic-color roles). `action_id` optional —
  tappable when present, display-only when `None`.
- `Component::SectionedActionList { id, sections: Vec<Section> }` —
  structured menu primitive (multiple labeled groups of tappable
  items). Distinct from `Component::ActionList` (flat menu); section
  grouping is structural, not a hint, so the discriminant lives at
  variant level. `Section { id, label, items: Vec<ActionListItem> }`.

Wire-type additions only — no engine emits them yet. Renderer
additions per frontend + engine emission flips (MoreEngine →
SectionedActionList; new `apply_sync_chrome_overlay` → Indicator)
land in follow-up work.

Refs `_private/docs/investigations/2026-05-28-core-screen-composition-surface.md`.

## [0.51.20] — 2026-05-28

### Added

- `AppEngine::apply_demo_contact_overlay` injects a
  `Component::Banner` on the Contacts screen when the onboarding
  demo is active. Reserved action id `"dismiss_demo_contact"` on
  the banner clears the demo via `Vauchi::dismiss_demo_contact`
  when pressed. Exfiltrates the previously-frontend-owned demo
  banner rendering (iOS `DemoContactCard`, ~90 LOC) to core per
  the shell-purity investigation. Re-uses the existing
  `Component::Banner` shape — no new variant.

## [0.51.19] — 2026-05-28

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
