<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
<!-- markdownlint-disable MD013 line-length-backtick-exempt -->

# B1 — `VauchiPlatform` → `PlatformAppEngine` Classification

| Field         | Value |
|---------------|-------|
| Phase         | B1 (Paper exercise — no code changes) |
| Created       | 2026-04-28 |
| Owner         | Mattia Egloff |
| Parent plan   | `_private/docs/problems/2026-04-28-collapse-vauchi-platform-into-app-engine/implementation-plan.md` |
| Status        | classification done — investigation gap-count revised |

This document resolves Open Question 4 from the implementation plan: it
walks every public method on the legacy `VauchiPlatform` UniFFI surface
against the current `PlatformAppEngine` surface and the `UserAction`
dispatch enum, classifying each method as `direct`, `action`, or `gap`.

## Headline Numbers (revised)

| Source | Count | Notes |
|--------|-------|-------|
| `VauchiPlatform` `pub fn` count (across `src/lib.rs` + `src/mobile_*.rs`) | **220** | Authoritative count, `rg '^\s*pub (async )?fn ' core/vauchi-platform/src/{lib,mobile_*}.rs` |
| `PlatformAppEngine` `pub fn` count | **34** (30 production + 4 test-only) | `core/vauchi-platform/src/platform_app_engine.rs` |
| `UserAction` enum variants | **12** | `core/vauchi-app/src/ui/action.rs` |

The `investigation.md` summary table claimed **121 methods ready** and
**18 needing API**. That classification was wrong: it counted "exists on
the legacy `VauchiPlatform` impl blocks" as "ready to migrate", which is
the opposite of B1's question. **The actual gap count is closer to 200,
not 18.**

## Methodology

For each public method on `VauchiPlatform` we ask three questions in
order, and stop at the first `yes`:

1. **Direct?** — does `PlatformAppEngine` expose a same-named (or
   trivially-renamed) method via `#[uniffi::export]`?
2. **Action-dispatchable?** — would the call site naturally migrate to
   `dispatch_action(UserAction::X { ... })` because the operation is
   already triggered by an `action_id` on a known active screen, AND
   the frontend call site is a post-button-press handler (not a
   programmatic / background path)?
3. **Gap.** — neither route exists; needs new `PlatformAppEngine`
   surface (or, separately, a new `UserAction` variant if we choose the
   action-dispatch path).

The `UserAction` enum today contains only generic UI events
(`TextChanged`, `ItemToggled`, `ActionPressed { action_id }`,
`SearchChanged`, etc.). Specifically, **no `UserAction` variant carries
domain payloads** — there is no `UserAction::AddContactField {
contact_id, field }` or `UserAction::CreateRecoveryClaim`. This means
"action-dispatchable" is only available for call sites that are already
button-driven and where the active screen's `WorkflowEngine` already
handles the `action_id`. None of the iOS/Android `VauchiRepository`'s
292 call sites match that pattern today — they are all programmatic
calls invoked from non-screen contexts (init, deep links, BG sync,
exports, NFC handlers).

## Direct Methods Already on `PlatformAppEngine`

These are the methods that `VauchiRepository.swift` /
`VauchiRepository.kt` already calls (or could trivially redirect to)
without any new core surface. **Phase C migration is a pure rename
for these.**

| `VauchiPlatform` method | `PlatformAppEngine` equivalent | Notes |
|-------------------------|-------------------------------|-------|
| `has_identity` | `has_identity` | Direct match (`mobile_identity.rs:has_identity` ↔ `platform_app_engine.rs:has_identity`) |
| `set_platform_keychain` | (constructor parameter) | Becomes a `new_with_secure_key` constructor argument; not a runtime method |
| `is_certificate_pinning_enabled` | (none yet) | Currently only in `lib.rs` — promote to engine in Phase B |
| `set_pinned_certificate` | (none yet) | Same |
| `core_version` / `app_compat_version` | (none yet) | Static helpers; suitable for free functions, no engine needed |
| `export_storage_key` | (none yet) | Used only by debug paths; gap |

`PlatformAppEngine` itself adds a number of methods that are NOT on
`VauchiPlatform` (these are the Pure Humble UI surface):

- `boot`, `current_screen_json`, `current_screen_id`, `current_tab_id`,
  `tab_info`, `sidebar_items`, `available_screens_json`,
  `navigate_back_json` (forward `navigate_to_json` retired — CoreScreenIdMap
  rework S5, ADR-043 Am4)
- `handle_action_json`, `handle_hardware_event`, `advance_qr_frame_json`,
  `form_has_data`
- `handle_deep_link_uri`
- `invalidate_all`, `invalidate_screen_json`
- `set_network_online`,
  `periodic_sync_tick`, `periodic_sync_interval_seconds`,
  `periodic_sync_max_retries` (the work landed via the Round 2 P2 audit
  alongside ios!353 and android!340)
- `handle_app_backgrounded`, `poll_notifications`
- `set_device_capabilities_json`, `set_event_listener`
- `biometric_unlock_check`

These do not have `VauchiPlatform` counterparts — they are
new-and-better. They are listed here only so future readers don't
double-count them as "gap from the migration perspective": they are not
gaps, they are the destination.

**Direct methods relevant to migration: ~6.** (everything else on
`PlatformAppEngine` is the new surface, not a `VauchiPlatform`
replacement.)

## Action-Dispatchable Methods (existing `UserAction` variants)

**Zero.** No method on `VauchiPlatform` is reachable today via
`dispatch_action(UserAction::X { ... })` with an existing typed variant
that carries the right payload. The 12 `UserAction` variants are
UI-event types (text changed, item toggled, generic
`ActionPressed { action_id: String }`); they cannot transport
operation-specific arguments like `contact_id`, `field_payload`,
`voucher_b64`, etc. without a string-encoding hack that would defeat
the type-safety goal of the dispatch path.

The dispatch path COULD be made viable by introducing typed domain
actions (e.g., `UserAction::DomainCommand(DomainCommand)` with a
typed `DomainCommand` enum). That is a strategic redesign, not a B1
deliverable — flagged here as Recommendation R2.

## Gap — Methods Requiring New `PlatformAppEngine` Surface

Everything else. Per-file count:

| Source file | `pub fn` count | All gap (no engine equivalent) |
|-------------|----------------|--------------------------------|
| `mobile_contacts.rs` | 48 | yes |
| `lib.rs` | 46 | mostly (i18n, FAQs, helpers; many become free functions) |
| `mobile_delivery.rs` | 32 | yes |
| `mobile_identity.rs` | 19 | all but `has_identity` |
| `mobile_gdpr.rs` | 17 | yes |
| `mobile_visibility.rs` | 15 | yes |
| `mobile_security.rs` | 15 | yes |
| `mobile_device_link.rs` | 13 | yes |
| `mobile_recovery.rs` | 9 | yes |
| `mobile_nfc.rs` | 9 | yes |
| `mobile_device_link_session.rs` | 9 | session methods, not platform — keep as `MobileDeviceLinkSession` |
| `mobile_ble.rs` | 8 | session methods on `MobileBleExchangeSession` — same |
| `mobile_onboarding.rs` | 7 | yes |
| `mobile_animated_qr.rs` | ~~7~~ 0 (file retired 2026-05-23) | **retired Track A** — zero hand-written consumers of `MobileAnimatedQrSender` / `MobileAnimatedQrReceiver` / `MobileAnimatedQrConfig` in any frontend; core's `AnimatedQrSession` (in `vauchi-core::exchange::transport::animated_qr`) survives and is reached by `vauchi-app::ui::exchange` directly. ADR-043 Amendment 2 strict-equality allowlist updated in the same MR. |
| `mobile_ui.rs` | ~~4~~ 0 (file retired 2026-05-17) | **retired by slice 32c** — `MobileOnboardingWorkflow` collapsed into `PlatformAppEngine`. ADR-043 Amendment 2 codifies "one screen-driving UniFFI object per binding"; the new `peer_uniffi_objects_count` strict-equality test enforces. See `_private/docs/designs/2026-05-16-slice-32c-mobile-ui-retirement-design.md`. B1's earlier "partially superseded" note was the right read but didn't follow through. |
| `mobile_exchange.rs` | 4 | yes |
| `mobile_content.rs` | 4 | yes |
| `mobile_wifi_aware.rs` | ~~1~~ 0 (file retired 2026-05-23) | **retired Track A** — zero production consumers (UniFFI exports unused by any frontend); WiFi Aware is future hardware per ADR-031 and will follow the command/event pattern when added. |
| `mobile_import.rs` | 1 | yes |
| Others (1 each) | 2 | type conversions / display helpers |
| **Total** | **220** | **~190 unique callable methods need a destination** |

After excluding session-shaped types that legitimately remain as
their own `#[uniffi::Object]`s (`MobileBleExchangeSession`,
`MobileNfcHandshake`, `MobileDeviceLinkSession`),
and free helpers in `lib.rs` that should become free `#[uniffi::export]`
functions rather than `PlatformAppEngine` methods, the realistic count
of **new `PlatformAppEngine` direct methods** is ~150–170.

This is **6×–7× higher** than the 25 estimated in the implementation
plan (B2 = 9, B3 = 4, B4 = 12).

## Domain-by-Domain Verdict

Each row maps an investigation-table domain to its actual classification.

| Domain | Investigation said | B1 verdict | Notes |
|--------|--------------------|-----------|-------|
| Identity / Bootstrap | Ready (8) | 1 direct (`has_identity`), 7 gap | `create_identity`, `get_public_id`, `get_display_name`, `get_own_fingerprint` are all on `mobile_identity.rs`, not the engine |
| Contact Field Mutation | Ready (10) | 0 direct, 10 gap | All on `mobile_contacts.rs` |
| Contact CRUD | Ready (22) | 0 direct, 22 gap | All on `mobile_contacts.rs` |
| Visibility Labels | Ready (16) | 0 direct, 16 gap | All on `mobile_visibility.rs` |
| Field Visibility | Ready (6) | 0 direct, 6 gap | `mobile_visibility.rs` |
| Contact Notes | Ready (12) | 0 direct, 12 gap | `mobile_contacts.rs` |
| Contact Verification | Ready (8) | 0 direct, 8 gap | `mobile_contacts.rs` |
| Recovery | Needs API (18) | 0 direct, 18 gap | `mobile_recovery.rs`, plan B2 still correct |
| Delivery Records / Retry | Ready (14) | 0 direct, 14 gap | `mobile_delivery.rs` |
| Sync / Network / Relay | Ready (10) | 5 direct (P2-C/D from Round 2 audit), 5 gap | `set_network_online`, `periodic_sync_tick` + 2 constants |
| Exchange (BLE/NFC/QR) | Ready (12) | 2 direct (`handle_hardware_event`, `advance_qr_frame_json`), 10 gap | Session creation methods stay as their own `Mobile*Session` types |
| Backup / Import / Export | Ready (10) | 0 direct, 10 gap | `mobile_delivery.rs` (export/import) + `mobile_import.rs` |
| Passcode / Duress | Ready (16) | 1 direct (`biometric_unlock_check`, P2-B audit fix), 15 gap | `mobile_security.rs` |
| Decoy Contacts | Ready (6) | 0 direct, 6 gap | `mobile_security.rs` |
| Hidden Contacts | Ready (6) | 0 direct, 6 gap | `mobile_contacts.rs` |
| Duplicate Detection | Ready (6) | 0 direct, 6 gap | `mobile_contacts.rs` |
| Emergency Broadcast | Needs API (6) | 0 direct, 6 gap | `mobile_security.rs`, plan B3 still correct |
| Shred (Security) | Ready (10) | 0 direct, 10 gap | `mobile_gdpr.rs` |
| GDPR / Deletion | Ready (6) | 0 direct, 6 gap | `mobile_gdpr.rs` |
| Consent | Ready (8) | 0 direct, 8 gap | `mobile_gdpr.rs` |
| Social Networks | Ready (8) | 0 direct, 8 gap | `mobile_contacts.rs` |
| Content Updates | Ready (6) | 0 direct, 6 gap | `mobile_content.rs` |
| Aha Moments | Ready (10) | 0 direct, 10 gap | `mobile_identity.rs` |
| Demo Contact | Ready (14) | 0 direct, 14 gap | `mobile_identity.rs` |
| Device Linking | Needs API (20) | 0 direct, 20 gap | `mobile_device_link.rs` + `mobile_device_link_session.rs`, plan B4 still correct |
| Certificate Pinning | Ready (4) | 2 direct (top-level `lib.rs`), 2 gap | Promote to engine |

**Re-totalled gap count: ~205 methods** (excluding session types).

## Strategic Implications for Phase B

The implementation plan currently sizes Phase B as **~7 focused days,
~25 new methods, 1 core MR + 3 binding bumps**. That assumed only
Recovery (9), Emergency Broadcast (4), Device Linking (12) needed new
surface. **B1 finds the gap is ~8× larger.**

Three strategic options for the user to choose between:

### R1 — Full pass-through expansion (sized by B1)

Add ~150–170 new `pub fn` wrappers on `PlatformAppEngine`, each
delegating to the matching `Vauchi` API and emitting cache invalidation.
Phase B effort grows to **~50–60 focused days**, 8–10 binding bumps.

Pros: Phase C migration stays as the planned mechanical rename.
Cons: enormous engine surface; doubles the UniFFI binding size; every
new core feature now has two registration sites (the `Vauchi` impl + the
engine wrapper).

### R2 — Typed `DomainCommand` dispatch (recommended)

Introduce a single dispatch entry point on `PlatformAppEngine`:

```rust
pub enum DomainCommand {
    AddContactField { contact_id: String, field: MobileFieldPayload },
    PanicShred,
    CreateRecoveryClaim,
    GrantConsent { feature: String, granted: bool },
    /* ~150 variants — one per VauchiPlatform method */
}

#[uniffi::method]
pub fn dispatch_domain_command(&self, cmd: DomainCommand)
    -> Result<DomainCommandResult, MobileError> { ... }
```

`DomainCommandResult` is a sum type of every legitimate return shape.
Phase B work becomes "define the enum + the dispatch impl + cache
invalidation". Phase C migration shape becomes
`vauchi.addField(contactId, payload)` →
`appEngine.dispatchDomainCommand(.addContactField(contactId, payload))`.

Pros: single FFI surface (the original audit goal); ~7 days for the
enum + dispatch matches the original Phase B estimate; future features
add a single variant, not a method.
Cons: enum proliferation; UniFFI generates a chunky enum; result-type
unification takes care.

### R3 — Hybrid: direct methods for high-traffic domains, dispatch for the long tail

Keep the planned B2/B3/B4 direct-method bumps for Recovery, Emergency
Broadcast, Device Linking (heavily-used, state-changing, deserve their
own methods). Use R2's `dispatch_domain_command` for the long tail
(~180 methods). Best-of-both: known high-traffic domains stay
discoverable + properly typed; long tail collapses to one dispatch
entry.

## Recommendation

**R3 (hybrid).** Keeps the existing Phase-B plan for the three priority
domains (B2 Recovery, B3 Emergency Broadcast, B4 Device Linking) — those
get individual typed methods because they are user-facing and warrant
discoverability. Adds a **B7** phase: introduce
`PlatformAppEngine::dispatch_domain_command(DomainCommand)` covering the
remaining ~180 methods in one MR.

Estimated revised Phase B: **~12 focused days** (was ~7) — adds ~5 days
for B7 (enum definition + dispatch impl + per-variant cache
invalidation matrix) but does not multiply the binding bump count.
Phase C effort is unchanged.

## Out of Scope for B1

- Picking R1 vs R2 vs R3 — that's a design decision for the user before
  B2 starts.
- Per-method cache-invalidation matrix — deferred to whichever option
  is chosen.
- `UserAction` enum redesign — orthogonal to this collapse; `UserAction`
  remains a UI-event type, `DomainCommand` (if R2/R3) is the
  programmatic-command type.

## Verification

- [x] `pub fn` count from `core/vauchi-platform/src/{lib,mobile_*}.rs`:
  220 unique names. Re-runnable:
  `for f in core/vauchi-platform/src/{lib,mobile_*}.rs; do rg '^\s*pub (async )?fn (\w+)' "$f" -or '$2'; done | sort -u | wc -l`.
- [x] `PlatformAppEngine` method count: 34 (30 production + 4 test).
  Re-runnable: `rg -c '^\s*pub (async )?fn ' core/vauchi-platform/src/platform_app_engine.rs`.
- [x] `UserAction` variant count: 12. Source:
  `core/vauchi-app/src/ui/action.rs:15-73`.
- [x] No code changes — pure paper exercise per implementation-plan.md
  Phase B1 contract.
