<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->

# Changelog

All notable changes to vauchi-core are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/).

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
