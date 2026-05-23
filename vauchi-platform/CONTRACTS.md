<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- markdownlint-disable MD013 line-length-backtick-exempt -->

# `vauchi-platform` UniFFI Surface — Migration Contracts

| Field            | Value                                              |
|------------------|----------------------------------------------------|
| Phase            | B6 (CC-05 — `_private/docs/problems/2026-04-28-collapse-vauchi-platform-into-app-engine/`) |
| Created          | 2026-04-28                                         |
| Owner            | Mattia Egloff                                      |
| Companion docs   | [`B1-CLASSIFICATION.md`](B1-CLASSIFICATION.md), [`STRUCTURE.md`](STRUCTURE.md) |

This file is the **per-method migration contract** for the
`VauchiPlatform → PlatformAppEngine` collapse. Phase-C frontend MRs
treat each row as a moving target — the iOS / Android `VauchiRepository`
call site for each method must be rewritten to use the
**Replacement** column. Once every consumer has migrated, the row
moves to "[REMOVED]" status and the underlying `VauchiPlatform`
method is deleted in Phase D3.

For the strategic context (R3 hybrid, R1/R2 alternatives), see
`B1-CLASSIFICATION.md`. For the per-domain Phase-C MR sequence, see
the [`plan.md`](https://gitlab.com/vauchi/private/-/blob/main/docs/problems/2026-04-28-collapse-vauchi-platform-into-app-engine/plan.md)
in the private repo.

## Replacement legend

- `PlatformAppEngine::X` — typed direct method, exists today (B2 / B3 / B4 ship).
- `dispatch_domain_command(DomainCommand::X { ... })` — typed dispatch
  variant on the long-tail enum (B7 will introduce; not on `main`
  yet at the time of this writing).
- *(stays on session type)* — method is on a non-`VauchiPlatform`
  type (`MobileBleExchangeSession`, `MobileNfcHandshake`,
  `MobileDeviceLinkSession`, `MobileMultiStageSession`, and the
  additional session peers
  enumerated by `peer_uniffi_objects_count` in
  `core/vauchi-app/tests/it/humble_surface_contract_tests.rs`).
  These remain as their own `#[uniffi::Object]`s — they are not
  part of the collapse. `MobileOnboardingWorkflow` was removed
  from this set by slice 32c (2026-05-17): it was screen-shaped
  (UserAction → ActionResult + ScreenModel), not session-shaped
  (no `ExchangeHardwareEvent` consumption), and collapsed into
  `PlatformAppEngine`. See ADR-043 Amendment 2 + the design pass
  at `_private/docs/designs/2026-05-16-slice-32c-mobile-ui-retirement-design.md`.
- *(free function)* — method becomes a top-level `#[uniffi::export]`
  function (e.g. `core_version`, `is_safe_url`, FAQ helpers, theme
  helpers). Not bound to any object.
- *(constructor parameter)* — passed at engine construction, no
  runtime method.

## Status legend

- **MIGRATED** — `PlatformAppEngine` equivalent exists on `main`;
  call site rewrite is unblocked. `VauchiPlatform` method carries
  `#[deprecated]`.
- **PENDING B7** — replacement is a `DomainCommand` variant; lands
  with B7. Frontends keep using the legacy method until then.
- **NOT MIGRATED — STAYS** — method legitimately remains on its
  current type (session methods, free functions, constructor
  params). Not part of the collapse.
- **PRE-ORCHESTRATOR — KEEP UNTIL D3** — pre-Phase-2d device-link
  legacy methods. They stay on `VauchiPlatform` (deprecated) until
  Windows + iOS + Android have all migrated their legacy call
  sites in Phase C8.

## Domain Tables

### Identity / Bootstrap

Source: `mobile_identity.rs` + `lib.rs` constructors.

| `VauchiPlatform` method | Replacement | Status | Notes |
|------------------------|-------------|--------|-------|
| `has_identity` | `PlatformAppEngine::has_identity` | **MIGRATED** | Already on engine since v0.30.0 |
| `create_identity` | `dispatch_domain_command(DomainCommand::CreateIdentity { display_name })` | **PENDING B7** | Bootstrap path; today reachable via onboarding `UserAction`s |
| `get_public_id` | `dispatch_domain_command(DomainCommand::GetPublicId)` | **PENDING B7** | Read |
| `get_display_name` | `dispatch_domain_command(DomainCommand::GetDisplayName)` | **PENDING B7** | Read |
| `get_own_fingerprint` | `dispatch_domain_command(DomainCommand::GetOwnFingerprint)` | **PENDING B7** | Read |
| `aha_moments_seen_count` | `dispatch_domain_command(DomainCommand::AhaMomentsSeenCount)` | **PENDING B7** | Read |
| `aha_moments_total_count` | `dispatch_domain_command(DomainCommand::AhaMomentsTotalCount)` | **PENDING B7** | Read |
| `has_seen_aha_moment` | `dispatch_domain_command(DomainCommand::HasSeenAhaMoment { id })` | **PENDING B7** | Read |
| `try_trigger_aha_moment` | `dispatch_domain_command(DomainCommand::TryTriggerAhaMoment { id })` | **PENDING B7** | Write |
| `try_trigger_aha_moment_with_context` | `dispatch_domain_command(DomainCommand::TryTriggerAhaMomentWithContext { id, ctx })` | **PENDING B7** | Write |
| `reset_aha_moments` | `dispatch_domain_command(DomainCommand::ResetAhaMoments)` | **PENDING B7** | Write |
| `init_demo_contact_if_needed` | `dispatch_domain_command(DomainCommand::InitDemoContactIfNeeded)` | **PENDING B7** | Write |
| `get_demo_contact` | `dispatch_domain_command(DomainCommand::GetDemoContact)` | **PENDING B7** | Read |
| `get_demo_contact_state` | `dispatch_domain_command(DomainCommand::GetDemoContactState)` | **PENDING B7** | Read |
| `is_demo_update_available` | `dispatch_domain_command(DomainCommand::IsDemoUpdateAvailable)` | **PENDING B7** | Read |
| `trigger_demo_update` | `dispatch_domain_command(DomainCommand::TriggerDemoUpdate)` | **PENDING B7** | Write |
| `dismiss_demo_contact` | `dispatch_domain_command(DomainCommand::DismissDemoContact)` | **PENDING B7** | Write |
| `auto_remove_demo_contact` | `dispatch_domain_command(DomainCommand::AutoRemoveDemoContact)` | **PENDING B7** | Write |
| `restore_demo_contact` | `dispatch_domain_command(DomainCommand::RestoreDemoContact)` | **PENDING B7** | Write |

### Onboarding

Source: `mobile_onboarding.rs`. Currently reachable via the
existing `UserAction` flow (`ActionPressed { action_id }`,
`TextChanged`, etc.) — these methods are programmatic
shortcuts the frontend rarely needs.

| `VauchiPlatform` method | Replacement | Status |
|------------------------|-------------|--------|
| `advance_onboarding` | `dispatch_action(UserAction::ActionPressed { ... })` (already on engine) | **MIGRATED** (use `handle_action_json`) |
| `current_onboarding_step` | *(read from `current_screen_json`)* | **MIGRATED** |
| `display_name_suggestions` | `dispatch_domain_command(DomainCommand::DisplayNameSuggestions)` | **PENDING B7** |
| `get_onboarding_progress` | *(read from `current_screen_json`)* | **MIGRATED** |
| `is_onboarding_complete` | `PlatformAppEngine::has_identity` (≈) | **MIGRATED** |
| `reset_onboarding` | `dispatch_domain_command(DomainCommand::ResetOnboarding)` | **PENDING B7** |
| `skip_onboarding_step` | `dispatch_action(UserAction::ActionPressed { action_id: "skip" })` | **MIGRATED** |

### Contact Field Mutation + CRUD + Verification + Notes (`mobile_contacts.rs`)

48 methods on the legacy struct. **All PENDING B7** — they will be
folded into `DomainCommand::Contact*` variants. The frontend `iOS
VauchiRepository.add_field(...)` etc. call sites stay on
`VauchiPlatform.X` until the B7 MR lands.

Representative subset (the full list is enumerable via
`rg '^\s*pub (async )?fn (\w+)' core/vauchi-platform/src/mobile_contacts.rs -or '$2' | sort -u`):

| Method | Status |
|--------|--------|
| `get_own_card`, `add_field`, `update_field`, `remove_field`, `set_display_name`, `set_own_avatar`, `clear_own_avatar` | PENDING B7 |
| `list_contacts`, `list_contacts_paginated`, `get_contact`, `search_contacts`, `contact_count`, `archive_contact`, `list_archived_contacts`, `hard_delete_imported_contact` | PENDING B7 |
| `find_duplicates`, `dismiss_duplicate`, `merge_contacts` | PENDING B7 |
| `verify_contact`, `set_proposal_trusted`, `is_field_visible_to_contact` | PENDING B7 |
| `set_contact_note`, `get_contact_note`, `delete_contact_note`, `set_contact_field_note`, `get_contact_field_notes`, `delete_contact_field_note` | PENDING B7 |
| `clear_contact_custom_avatar`, `clear_contact_nickname`, `get_contact_custom_avatar`, `get_contact_display_options`, `contact_detail_footer_action_id` | PENDING B7 |
| `hide_contact`, `list_hidden_contacts`, `list_social_networks`, `get_profile_url` | PENDING B7 |
| `trust_contact_for_recovery` | `PlatformAppEngine::trust_contact_for_recovery` | **MIGRATED** (B2) |
| `untrust_contact_for_recovery` | `PlatformAppEngine::untrust_contact_for_recovery` | **MIGRATED** (B2) |
| `trusted_contact_count` | `PlatformAppEngine::trusted_contact_count` | **MIGRATED** (B2) |

### Visibility Labels + Field Visibility (`mobile_visibility.rs`)

15 methods. **All PENDING B7.**

`add_contact_to_group`, `create_label`, `delete_label`,
`get_groups_for_contact`, `get_label`, `get_suggested_labels`,
`hide_field_from_contact`, `is_field_visible_to_contact`,
`list_labels`, `remove_contact_field_override`,
`remove_contact_from_group`, `rename_label`,
`set_contact_field_override`, `set_group_field_visibility`,
`show_field_to_contact`.

### Recovery (`mobile_recovery.rs`)

| `VauchiPlatform` method | Replacement | Status |
|------------------------|-------------|--------|
| `create_recovery_claim` | `PlatformAppEngine::create_recovery_claim` | **MIGRATED** (B2) |
| `parse_recovery_claim` | `PlatformAppEngine::parse_recovery_claim` | **MIGRATED** (B2) |
| `create_recovery_voucher` | `PlatformAppEngine::create_recovery_voucher` | **MIGRATED** (B2) |
| `add_recovery_voucher` | `PlatformAppEngine::add_recovery_voucher` | **MIGRATED** (B2) |
| `get_recovery_status` | `PlatformAppEngine::get_recovery_status` | **MIGRATED** (B2) |
| `get_recovery_proof` | `PlatformAppEngine::get_recovery_proof` | **MIGRATED** (B2) |
| `verify_recovery_proof` | `dispatch_domain_command(DomainCommand::VerifyRecoveryProof { proof_b64 })` | **PENDING B7** |
| `upload_guardian_entries` | `dispatch_domain_command(DomainCommand::UploadGuardianEntries)` | **PENDING B7** |
| `save_recovery_response` | `dispatch_domain_command(DomainCommand::SaveRecoveryResponse { ... })` | **PENDING B7** |

### Emergency Broadcast (`mobile_security.rs`)

| `VauchiPlatform` method | Replacement | Status |
|------------------------|-------------|--------|
| `configure_emergency_broadcast` | `PlatformAppEngine::configure_emergency_broadcast` | **MIGRATED** (B3) |
| `send_emergency_broadcast` | `PlatformAppEngine::send_emergency_broadcast` | **MIGRATED** (B3) |
| `get_emergency_config` | `PlatformAppEngine::get_emergency_config` | **MIGRATED** (B3) |
| `disable_emergency_broadcast` | `PlatformAppEngine::disable_emergency_broadcast` | **MIGRATED** (B3) |

### Passcode + Duress + Decoy (rest of `mobile_security.rs`)

11 methods. **All PENDING B7.**

`setup_app_password`, `setup_duress_password`, `authenticate`,
`is_password_enabled`, `is_duress_enabled`, `disable_duress`,
`configure_duress_alerts`, `get_duress_settings`,
`add_decoy_contact`, `list_decoy_contacts`, `delete_decoy_contact`.

### Sync + Delivery + Backup + Import (`mobile_delivery.rs`, `mobile_import.rs`)

33 methods. **All PENDING B7** except for the 3 lifecycle methods
already on `PlatformAppEngine`:

| `VauchiPlatform` method | Replacement | Status |
|------------------------|-------------|--------|
| *(all sync/delivery methods on `mobile_delivery.rs`)* | `dispatch_domain_command(DomainCommand::Delivery*)` | PENDING B7 |
| *(`export_backup`, `import_backup`, `export_full_backup`, `import_full_backup`)* | `dispatch_domain_command(DomainCommand::Backup*)` | PENDING B7 |
| `import_contacts_from_vcf` | `dispatch_domain_command(DomainCommand::ImportContactsFromVcf { vcf_data })` | PENDING B7 |
| *(implicit)* `periodic_sync_tick` etc. | `PlatformAppEngine::periodic_sync_tick` + interval/retry | **MIGRATED** (Round 2 P2-C) |
| *(implicit)* network state | `PlatformAppEngine::set_network_online` / `is_network_online` | **MIGRATED** (Round 2 P2-D) |

### Exchange (`mobile_exchange.rs`, `mobile_ble.rs`, `mobile_nfc.rs`)

> `mobile_wifi_aware.rs` retired 2026-05-23 (Track A) — zero production consumers; future WiFi Aware support follows ADR-031 command/event pattern.
> `mobile_animated_qr.rs` retired 2026-05-23 (Track A) — zero hand-written consumers; core's `AnimatedQrSession` survives at `vauchi-core::exchange::transport::animated_qr`.

Mixed: high-level entry points migrate, session types stay.

| Method / Type | Replacement | Status |
|---------------|-------------|--------|
| `VauchiPlatform::create_qr_exchange`, `create_qr_exchange_manual`, `finalize_exchange` | `dispatch_domain_command(DomainCommand::Exchange*)` | PENDING B7 |
| `VauchiPlatform::create_multistage_session` | `PlatformAppEngine::handle_action_json` (auto-managed by engine) | **MIGRATED** (Pair 4) |
| `MobileBleExchangeSession::*` | *(stays on session type)* | NOT MIGRATED — STAYS |
| `MobileAnimatedQrSender::*`, `MobileAnimatedQrReceiver::*`, `MobileAnimatedQrConfig` | *(file retired)* | **RETIRED 2026-05-23 (Track A)** — zero hand-written consumers in any frontend; `mobile_animated_qr.rs` deleted, core's `AnimatedQrSession` survives at `vauchi-core::exchange::transport::animated_qr`. |
| `MobileNfcHandshake::*` | *(stays on session type)* | NOT MIGRATED — STAYS |
| `VauchiPlatform::create_nfc_initiator`, `create_nfc_responder` | `dispatch_domain_command(DomainCommand::NfcCreate*)` | PENDING B7 |
| `MobileMultiStageSession::*` | *(stays on session type)* | NOT MIGRATED — STAYS |

### Device Linking (`mobile_device_link.rs`, `mobile_device_link_session.rs`)

| `VauchiPlatform` method | Replacement | Status |
|------------------------|-------------|--------|
| `get_devices` | `PlatformAppEngine::get_devices` | **MIGRATED** (B4) |
| `device_count` | `PlatformAppEngine::device_count` | **MIGRATED** (B4) |
| `is_primary_device` | `PlatformAppEngine::is_primary_device` | **MIGRATED** (B4) |
| `unlink_device` | `PlatformAppEngine::unlink_device` | **MIGRATED** (B4) |
| `generate_device_link_qr` | `PlatformAppEngine::generate_device_link_qr` | **MIGRATED** (B4) |
| `parse_device_link_qr` | `PlatformAppEngine::parse_device_link_qr` | **MIGRATED** (B4) |
| `create_device_link_session_initiator` | `PlatformAppEngine::create_device_link_session_initiator` | **MIGRATED** (B4) |
| `start_device_link` | *(removed — pre-orchestrator)* | PRE-ORCHESTRATOR — KEEP UNTIL D3 |
| `start_device_join` | *(removed — pre-orchestrator)* | PRE-ORCHESTRATOR — KEEP UNTIL D3 |
| `send_device_link_request` | *(removed — pre-orchestrator)* | PRE-ORCHESTRATOR — KEEP UNTIL D3 |
| `listen_for_device_link_request` | *(removed — pre-orchestrator)* | PRE-ORCHESTRATOR — KEEP UNTIL D3 |
| `send_device_link_response` | *(removed — pre-orchestrator)* | PRE-ORCHESTRATOR — KEEP UNTIL D3 |
| `MobileDeviceLinkSession::*` | *(stays on session type)* | NOT MIGRATED — STAYS |

### GDPR + Consent + Shred (`mobile_gdpr.rs`)

16 methods. **All PENDING B7.**

`cancel_identity_deletion`, `cancel_shred`, `check_consent`,
`execute_identity_deletion`, `export_gdpr_data`,
`get_consent_records`, `get_consent_status`, `get_deletion_state`,
`grant_consent`, `hard_shred`, `panic_shred`, `revoke_consent`,
`schedule_identity_deletion`, `set_platform_keychain` *(constructor parameter)*,
`shred_status`, `soft_shred`.

`verify_shred` retired 2026-05-23 (Track A — zero hand-written
consumers; `MobileShredVerification` record + `From` impl retired
alongside).

`set_platform_keychain` is a constructor parameter on
`PlatformAppEngine::new` (the secure-key seam) — not a runtime
method on the engine.

### Content Updates (`mobile_content.rs`)

4 methods. **All PENDING B7.**

`apply_content_updates`, `check_content_updates`,
`is_content_updates_supported`, `reload_social_networks`.

### Free Helpers + i18n + FAQ + Themes (`lib.rs` non-method exports)

46 methods on `lib.rs` — most are free helpers that should NOT
become `PlatformAppEngine` methods. They become top-level
`#[uniffi::export]` functions in B7 (or stay as-is).

| Method | Replacement | Status |
|--------|-------------|--------|
| `core_version`, `app_compat_version`, `is_safe_url`, `is_allowed_scheme`, `is_blocked_scheme`, `is_valid_relay_url`, `parse_locale_code`, `check_password_strength`, `classify_device_type` | *(free function)* | NOT MIGRATED — STAYS (already free helpers) |
| `compute_confirmation_code`, `prepare_confirmation`, `proximity_challenge`, `confirm_link_manual`, `confirm_link_ultrasonic`, `expires_at`, `qr_data`, `create_request`, `finish_join`, `identity_fingerprint` | *(stays on session type)* | NOT MIGRATED — STAYS |
| `widget_panic_shred` | `dispatch_domain_command(DomainCommand::PanicShred)` | PENDING B7 |
| `is_certificate_pinning_enabled`, `set_pinned_certificate` | `PlatformAppEngine::is_certificate_pinning_enabled` / `set_pinned_certificate` | PENDING B7 (typed methods, low priority) |
| `export_storage_key`, `generate_storage_key` | *(stays — debug / setup)* | NOT MIGRATED — STAYS |
| `get_string`, `get_string_with_args`, `get_aha_moment_localized`, `get_locale_info`, `get_available_locales`, `init_locales` | *(free function)* | NOT MIGRATED — STAYS |
| `get_available_themes`, `get_default_theme_id`, `get_theme` | *(free function)* | NOT MIGRATED — STAYS |
| `get_faqs`, `get_faqs_localized`, `get_faqs_by_category`, `get_faqs_by_category_localized`, `get_faq_by_id`, `get_faq_by_id_localized`, `get_help_categories`, `search_faqs`, `search_faqs_localized` | *(retired)* | **RETIRED 2026-05-23 (Track A)** — zero hand-written consumers in any frontend; 9 `#[uniffi::export]` free fns + `MobileFaqItem` / `MobileHelpCategory` / `MobileHelpCategoryInfo` deleted, core's `vauchi_app::help::*` survives for future `HelpWorkflowEngine` use. |
| `save_test_contact`, `save_test_delivery_record` | `PlatformAppEngineTestHelpers` trait | **MIGRATED** to `#[doc(hidden)] pub trait PlatformAppEngineTestHelpers` (slice 32g 2026-05-17 + slice 32h.Ph2 2026-05-18); trait impls don't count toward audit |
| `new`, `new_with_secure_key` | `PlatformAppEngine::new` | **MIGRATED** (covered by engine constructor) |

## Summary

| Status | Count | Notes |
|--------|-------|-------|
| MIGRATED (typed direct method) | 20 | 9 recovery (B2) + 4 emergency broadcast (B3) + 7 device linking (B4) |
| MIGRATED (already on engine pre-collapse) | ~10 | `has_identity`, `boot`, `current_screen_json`, navigation, lifecycle, `periodic_sync_tick`, `set_network_online`, `biometric_unlock_check`, etc. |
| PENDING B7 (DomainCommand long-tail) | ~150 | All `mobile_contacts`, `mobile_visibility`, `mobile_gdpr`, the rest of `mobile_security`, `mobile_content`, `mobile_delivery`, parts of `mobile_recovery`, `mobile_identity`, exchange entry points |
| NOT MIGRATED — STAYS (session types) | ~30 | `MobileBleExchangeSession`, `MobileNfcHandshake`, `MobileDeviceLinkSession`, `MobileMultiStageSession`, and the other session peers — full enumeration pinned by `peer_uniffi_objects_count` in `core/vauchi-app/tests/it/humble_surface_contract_tests.rs` (ADR-043 Am.2). `MobileOnboardingWorkflow` was retired by slice 32c (2026-05-17, screen-shaped — not a session peer). |
| NOT MIGRATED — STAYS (free functions) | ~40 | i18n, FAQ, theme, validation, helper functions on `lib.rs` |
| PRE-ORCHESTRATOR — KEEP UNTIL D3 | 5 | `start_device_link`, `start_device_join`, `send_device_link_request`, `listen_for_device_link_request`, `send_device_link_response` — superseded by `MobileDeviceLinkSession`, retained for the deprecation cycle |

## Phase-C Sequencing — Per-Method Ownership

Each Phase-C MR (C1 … C8 in the plan's task DAG) takes ownership
of one row group:

- **C1** (Identity + Bootstrap + Contact Field Mutation): rows in
  *Identity / Bootstrap* + the field-mutation rows in
  *Contact ... Notes*. Drives **B7 ordering** — needs the
  matching `DomainCommand` variants live before C1 starts on a
  consumer repo.
- **C2** (Contact CRUD + Verification + Notes): the rest of the
  contacts table.
- **C3** (Visibility Labels + Field Visibility): the
  *Visibility Labels* table.
- **C4** (Sync + Delivery + Retry): the *Sync ...* table.
- **C5** (Backup + Decoy + Hidden + Duplicates): rows split
  across *Sync*, *Passcode + Duress + Decoy*, and *Contact CRUD*.
- **C6** (Passcode + Duress + GDPR + Consent + Shred): merges
  *Passcode + Duress + Decoy* and *GDPR + Consent + Shred*.
- **C7** (Recovery + Emergency Broadcast): unblocked once B2 + B3
  ship; first domain in C-order that touches `PlatformAppEngine`
  rather than `dispatch_domain_command`.
- **C8** (Exchange + Social + Content + Aha + Demo + Cert +
  Device Linking): largest bundle; lands last; merges 7 tables.

When a consumer migration MR ships, update the row's status to
`MIGRATED` (mention the MR id) so this file becomes the source of
truth for the deletion gate in D3.

## Verification Commands

```sh
# Re-derive the per-file VauchiPlatform method count.
for f in core/vauchi-platform/src/{lib,mobile_*}.rs; do
    rg '^\s*pub (async )?fn (\w+)' "$f" -or '$2'
done | sort -u | wc -l

# Re-derive the PlatformAppEngine method count.
rg -c '^\s*pub (async )?fn ' core/vauchi-platform/src/platform_app_engine.rs

# List active consumers of any VauchiPlatform method (CC-05 gate).
just consumers VauchiPlatform
```
