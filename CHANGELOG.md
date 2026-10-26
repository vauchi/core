<!-- SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> -->
<!-- SPDX-License-Identifier: AGPL-3.0-or-later -->

# Changelog

All notable changes to vauchi-core are documented here.
Format follows [Keep a Changelog](https://keepachangelog.com/).

## [Unreleased]

## [0.90.0] — 2026-10-26

### Added

- `Event::WakeupFired` lets a shell report a timer firing through the
  canonical event channel. Core returns due notifications as
  `PostNotification` commands, preserving their opaque replacement IDs,
  together with the next wakeup and pending presentation commands. A shell
  must execute native notification commands before adopting this event;
  legacy notification collection remains compatible.
- Every surface carries the active theme's colours:
  `PresentationTokens.colors` (`PresentationColors`, `#rrggbb` by generic
  role; optional roles are `null` when the theme leaves them out). A pick
  in Core's Settings colours the next surface, and a duress unlock paints
  what a normal one does (vauchi/private#814).
- `PresentationEnvironmentChanged` takes an optional `color_scheme`
  (`light`/`dark`, omitted when absent). Under "follow system" Core paints
  the default theme for it, and a switch re-presents the surface at once.
- `Command::SaveRestorableState { state }`: the shell keeps `state` in
  its own per-device storage and hands it back unchanged as the
  render-context field `restorable_state`. It carries the language and
  theme pick, so the pick applies before storage opens, on the lock
  screen too. A shell that does not keep it yet still gets the pick from
  the vault.

### Changed

- `PreparedSurface::from_screen_in` takes the `RenderContext` instead of
  a `Locale`. Rust struct literals of `PresentationTokens`,
  `PresentationEnvironmentChanged` and `RenderContext` must supply the
  new fields, requiring this minor release (`lint:semver`).
  Strict surface readers must accept `tokens.colors`.

### Fixed

- Restore from Settings on a device that already has an identity no
  longer offers "Replace Identity?" and then always fails: a backup
  restores only into an empty app, and the wizard now says so and names
  Emergency Wipe (vauchi/private#819).

## [0.89.0] — 2026-10-25

### Changed

- `NotificationSpec` carries an optional opaque `notification_id` so native
  executors can preserve the existing OS replacement identity. Rust struct
  literals must supply this new field; older anonymous command JSON remains
  compatible. This source API change requires the new minor release.
- The shared presentation fixture includes an `effect_commands` corpus.
  Strict fixture readers must explicitly accept that field.

## [0.88.1] — 2026-10-25

### Fixed

- A background invalidation no longer throws the duress PIN and app
  password setups back: what was typed is what gets set
  (vauchi/private#809).
- A device-link host fails with `qr_expired` instead of completing a
  link after the QR expired, and the joiner still reads a response
  waiting at its deadline (vauchi/private#808).

### Added

- CI: `test:formal` also runs the Kani proofs of the CEK-wrapped card
  update decoder and Shamir interpolation, the Tamarin card-update
  model and the Quint device-link model (vauchi/private#757).

## [0.88.0] — 2026-10-24

### Added

- `Vauchi::fetch_path` reports whether fetches go through the relay's
  delayed path, or since when the relay withdrew it (migration v85),
  and Advanced > Network shows it (vauchi/private#628).
- A device-link host that declines, times out or finds the name taken
  tells the joiner at once, with a refusal only that ceremony can
  read, and the joiner's screen names the reason; a host that leaves
  the linking screen tells the joiner the link was canceled
  (vauchi/private#780).
- Platform callbacks are bound to a cancellable operation lifetime:
  stale observations, closed inboxes and resource-budget overruns are
  rejected.
- `AppEngine` offers native shells one opaque, coalescing
  presentation-invalidation receiver, so Core decides which events
  repaint; a replacement subscription or a dropped engine disconnects
  it, and `PresentationInvalidated` returns pending notifications with
  the renderer commands (vauchi/private#764).
- The new `vauchi-hardware` crate runs native discovery in one bounded
  worker that disposes every scan on shutdown, and Core decodes raw BLE
  advertisement observations, rejecting ambiguous ones.
- Core keeps the language and theme picked in Settings and restores
  them when an engine opens; before the first pick an app's own stored
  choice applies (vauchi/private#790).
- Dev bindings built with `dev-local-content` can trust a test content
  publisher key (`VAUCHI_DEV_CONTENT_PUBLISHER_KEY`) and send OHTTP to a
  local relay's own route (`VAUCHI_DEV_OHTTP_RELAY_URL`)
  (vauchi/private#733).
- CI: `test:formal` runs the Kani parser proofs and the Tamarin
  exchange, device-link and reciprocity-confirmation models
  (vauchi/private#757), the recovery-acceptance model of both rekey
  designs and the model of social recovery with online rekeying
  (vauchi/private#796); `test:core-32bit` also runs the 32-bit
  integration tests (vauchi/private#784).

### Fixed

- A voucher signed by either key of a recovery, the lost device's old
  key or the new one, is refused and never counted, so whoever holds
  the lost device cannot vouch for its own recovery
  (vauchi/private#797).
- On a device with biometrics, the device or login password no longer
  passes the OS check that guards export and the first app password:
  `CreateUserPresenceSecret` carries a `binding` (`biometry` or
  `device_credential`) that Core picks from `biometric_type`, export
  there needs an app password, and a lost or unavailable enrolment is
  refused and counted as a miss, with restore or re-link as the way
  back. Shells report a missing item as the new
  `UserPresenceSecretMissing` event. A duress session leaves the
  enrolment alone (vauchi/private#778).
- An open Edit Display Name, Edit Field or Edit Contact form keeps what
  was typed when a background sync invalidates the screen, instead of
  saving the stored value (vauchi/private#799).
- A guardian vouches only for a recovery claim that names one of their
  contacts and carries that contact's key; the vouch screen names the
  contact, and claims and vouchers carry the time
  (vauchi/private#708).
- A display name is limited to 100 characters everywhere: creating an
  identity refuses a longer one, renaming counts characters instead of
  bytes, and the exchange QR accepts any name the card accepts, up to
  400 bytes; `ExchangeQR::to_qr_image_string` now returns a `Result`,
  `SerializationFailed` for a payload past QR capacity, instead of
  panicking (vauchi/private#802).
- A new device refuses a linked seed whose identity is not the one the
  scanned QR names (vauchi/private#789).
- A QR timestamp near `u64::MAX` counts as expired instead of
  overflowing and aborting the app (vauchi/private#795).
- The sync payload and recovery voucher parsers compare a declared
  length without wrapping on a 32-bit `usize` (vauchi/private#784).
- Completed and backpressured hardware observations are kept, and a
  rejected payload is returned for a bounded retry.
- Tokio is required at 1.53.3 or later for RUSTSEC-2026-0386.
- The fuzz nightly replays coverage without ASan, whose odr check
  aborted every replay on bundled sqlite3 (vauchi/private#787).
- The parsers of untrusted input deny `clippy::arithmetic_side_effects`,
  `clippy::indexing_slicing` and `unwrap`/`expect`/`panic`: declared
  lengths slice with bounds checks, fixed headers are read as checked
  arrays, an imported contact yields no sync data instead of panicking,
  and chain and ratchet generations stop at `GenerationLimitExceeded`
  instead of wrapping (vauchi/private#761).
- A native callback is delivered serially, and BLE carriers keep their
  width with a disposal that is safe under cancellation.

### Changed

- Core is licensed AGPL-3.0-or-later; versions up to 0.87.0 stay
  available under GPL-3.0-or-later (vauchi/private#798).

## [0.87.0] — 2026-10-23

### Added

- Content updates: Core asks the relay for content gzipped, so locale
  files fit through the OHTTP relay; a setting chooses whether updates
  also run in the background; and `Command::ReloadTheme` asks the
  shell to reload its theme when an update brings new themes
  (vauchi/private#733).
- CI runs Core's library tests with a 32-bit `usize` (vauchi/private#767).

### Fixed

- A content update no longer rebuilds a live device-link screen, which
  dropped the code on screen and refused the host's Confirm
  (vauchi/private#758).
- A device name already linked is refused before the confirmation codes
  are shown, and the screen says to rename the new device
  (vauchi/private#671).
- A first app password waits for the OS user-presence check
  (vauchi/private#766).
- Unpadding checks the declared length without adding to it
  (vauchi/private#756).
- Shells read the themes in effect, so a CDN theme picked in Settings
  applies (vauchi/private#733).
- ureq is capped below 3.5, whose unversioned transport API TLS pinning
  uses (vauchi/private#752).

### Changed

- Relay trust is separated from the network and storage adapters, and
  core layer violations are removed with the public aliases kept
  (vauchi/private#760).

## [0.86.0] — 2026-10-22

### Added

- C ABI: `vauchi_app_set_render_context_json` lets linux-qt and windows
  push the system language, so Follow system renders it there too. A
  push made while storage is locked applies to the engine that opens
  (vauchi/private#721).
- Social networks come from data: the CDN's networks file, validated
  whole or not at all, else the bundled 38. A network resolves from its
  id, an alias or its name in any case; username rules and profile
  links come from that data, and a profile link names the host it opens
  (vauchi/private#734).
- A social field stores its network id, and shows the network's
  current name on the own card, contact detail and exchange preview
  (vauchi/private#734).
- A language published on the CDN needs no Core release: `Locale` holds
  any 2-3 letter ISO 639 language subtag, a locale file's `_meta` names
  its language, and Settings offers English first, then every loaded
  language and every language the verified manifest lists. A listed
  right-to-left language is not offered while no frontend mirrors its
  layout. Screens re-render once a picked language's file arrives
  (`AppEngine::reload_locales`, vauchi/private#733).
- Cached CDN content is stored gzipped, about a quarter of its size; a
  stored file that does not inflate, or inflates past the download
  limit, downloads again (vauchi/private#733).
- The app password clears a user-presence enrolment whose sealed secret
  was lost, so the device seals a fresh one the next time the OS check
  is used (`Vauchi::forget_user_presence_secret`, vauchi/private#739).
- Bindings: `MobileEvent` gains `UserPresenceSecretCreated`,
  `UserPresenceSecretRevealed` and `UserPresenceCheckCancelled`, so shells
  answer the user-presence commands through `hardware_event_json` instead
  of writing the JSON themselves (vauchi/private#740).

### Changed

- Protocol: card updates carry the network id, with payload version
  0x08 and the delta signature domain `vauchi-delta-v3`. Core 0.86.0
  and older Cores report each other's card updates as an unknown
  payload version; there is no backward compatibility
  (vauchi/private#734).
- Content: a manifest's locale entries may name their language
  (`name`, `english_name`, `is_rtl`). Older Cores fail to verify a
  manifest that carries them (vauchi/private#733).
- Bindings: `get_string`, `get_string_with_args`, `get_locale_info` and
  `get_aha_moment_localized` take a language code, and
  `parse_locale_code` returns one. A malformed code reads English
  (vauchi/private#733).

### Removed

- Bindings: `PlatformAppEngine.new` (the constructor taking a storage key
  the shell kept) and `setPlatformKeychain` are gone. Every shell opens
  Core with `openWithKeychain`, so every key that opens the data lives in
  the keychain and a shred deletes them all (vauchi/private#580).
- Bindings: `MobileLocale` is gone; locales cross the binding as
  language codes (vauchi/private#733).
- Core's table of five language names is gone; names come from the
  locale files (vauchi/private#733).

### Fixed

- Settings, and screens built before the shell first reports the system
  language, render in that language under Follow system; suggested
  groups render, and are created, in the onboarding language
  (vauchi/private#721).
- CDN locales and themes cached by an earlier run are in effect after a
  restart (vauchi/private#736).
- A downloaded locale or theme catalog that does not load is not cached
  and is retried; a cached one that does not load at start is dropped
  and downloads again (vauchi/private#747).

### Security

- Leaving a verified data export ends it: reopening Privacy, or Back
  from an export step, returns to the Privacy overview, and the next
  export asks for the app password or the OS check again. Before, a
  cached Privacy screen reopened on the format choice with no new check
  (vauchi/private#750).

## [0.85.0] — 2026-10-21

### Added

- Deleting the identity asks whether recovery stays possible: keep the
  relay's list of trusted contacts, or erase everything. Someone with no
  trusted contacts is not asked and erases everything. The choice waits
  out the seven-day grace period with the schedule, and linked devices
  keep it (vauchi/private#711).
- The deletion run purges the relay after the revocations and before the
  local wipe, with a fresh signed purge that carries the recovery choice
  in its signed bytes (`Vauchi::purge_relay_data`). A failed purge is
  logged and the local deletion continues (vauchi/private#711).
- Protocol: the purge request gains an optional `erase_guardian_set`,
  and recovery proof uploads carry the uploader's key, a timestamp and
  a signature (`recovery_store_owned`). Both are omitted when unset, so
  older relays read them as before (vauchi/private#711).
- Every data export asks the user to verify first: the session password,
  or without an app password the phone's own check through a
  keystore-sealed secret. The user then picks an encrypted file, vCard
  or JSON. The vCard export names each contact's Vauchi id and kind, and
  a re-import matches by it (vauchi/private#712).
- Tapping a contact's phone, email, website, profile or address opens
  it; Core builds the map link for the chosen provider and dialable
  phone links (vauchi/private#709).
- Background sync draws its gaps from an exponential schedule around
  15 min (1–60 min), and the shell reports the OS's background budget
  as `Event::BackgroundBudgetObserved { window_secs, runs }`. The
  scheduler tick names the next background run within that budget. The
  report answers with the next background run, so closing the app
  re-arms it, and a changed budget brings the sync deadline forward to
  that run (vauchi/private#603).

### Fixed

- TLS rejects certificate paths through the distrusted emSign
  intermediates: rustls-webpki 0.103.16, webpki-roots 1.0.10 and
  webpki-root-certs 1.0.10 (RUSTSEC-2026-0382, RUSTSEC-2026-0381,
  RUSTSEC-2026-0383).
- A second scanner's offer can no longer replace the first scanner's
  BLE commitment (vauchi/private#705), and a Glance displayer latches
  the link whose offer it accepts, not the last one to connect
  (vauchi/private#706).
- Delayed mailbox fetches keep their request's routing context: response
  tokens are accepted only from that request, and the context is bounded
  to the requested chunk and released afterwards.
- `bump:consumer-pins` retries a consumer push that was declined once
  (vauchi/private#704).

## [0.84.1] — 2026-10-18

### Fixed

- A completed BLE exchange releases its link: the initiator, which
  completes last, disconnects after its reciprocity ack. The dialling
  phone no longer keeps the link up on its Success screen, which made the
  other phone's next Glance stick on "Exchanging" (vauchi/private#673).
  iOS also needs vauchi-platform-swift 0.84.1, which decodes the
  addressed `BleDisconnect`.

## [0.84.0] — 2026-10-17

### Added

- Fetches and acknowledgements switch to the outer relay's delayed path
  when it advertises one (`Vauchi-Ohttp-Delay-Window`, 1–3600 s), so the
  gateway no longer sees a fetch when the client connects. The window is
  kept per outer relay (storage migration v83), a waiting sync waits it,
  and a relay that stops advertising returns its clients to the
  synchronous path. `RelayConfig::ohttp_delayed` stays as a force-on
  switch (vauchi/private#628).

### Fixed

- A link responder times out while retrieving the other card, not only
  while polling: an accepted expired link ends in "Link Failed" instead
  of waiting forever (vauchi/private#697).
- Contacts lists the contacts right after the search field; the "Also
  search" toggles follow the list, so a small phone no longer shows an
  apparently empty tab (vauchi/private#698).

## [0.83.0] — 2026-10-16

### Changed

- Password-protected full backups write version 6, preserving group
  visibility and presentation overrides, explicit contact sharing choices
  and contact lifecycle state. Restore still reads legacy version 3;
  older readers explicitly reject version 6.

### Fixed

- Full restore validates snapshot contents before changing storage and
  commits the restored state in one transaction. Failed imports leave a
  fresh installation uninitialized (vauchi/private#64).
- Recovery-proof upload validates the stored vouchers and posts the proof
  through the configured relay transport. A failed upload retains recovery
  progress for retry (vauchi/private#265).

## [0.82.1] — 2026-10-14

### Fixed

- `LinkExchange` and `LinkExchangeProgress` are exported from `api`
  (vauchi/private#670). 0.82.0 re-exported them only under the crate's
  private `api::vauchi` module, so no consumer could name them and the
  CLI's `vauchi exchange link` could not build against it.

## [0.82.0] — 2026-10-14

### Added

- Link-mode exchange as a domain API (vauchi/private#670):
  `Vauchi::start_link_exchange`, `accept_link_exchange` and
  `advance_link_exchange` run the ceremony the apps run, so a shell without
  the presentation protocol (the CLI) can share or accept a link.
- `Vauchi::resolve_own_field_id` names an own entry by id or by a label
  only one entry has; a label several entries share is refused with their
  ids (vauchi/private#651).
- `Vauchi::time_until_sync_due` reports how long until the next sync is
  due (vauchi/private#603).
- A Glance QR opened from the camera continues the exchange, and the
  consent screen names a link's sender, marked unchecked
  (vauchi/private#277).
- A confirmed contact answers a peer still waiting for confirmation with
  a new reciprocity payload (`0x07`, signed under its own domain), so a
  lost confirmation no longer leaves the peer re-sending on every sync
  (vauchi/private#663).
- Device-sync logs count journaled, sent, decoded and accepted items
  (vauchi/private#659).
- Multi-stage sessions log own and peer frame counts at finalize
  (vauchi/private#450).

### Changed

- The heartbeat wakes when the drawn sync gap ends instead of on its 30 s
  beat, so the exponential schedule reaches the relay (vauchi/private#603).
- A registry push is re-sent only after a message from the contact or a
  registry change, not on every sync (vauchi/private#662). Migration v82
  adds `registry_activation.repush_allowed`.
- One personal-note format: plain text, encrypted at rest by storage.
  `add_personal_note` stores it and journals it; notes written under the
  contact's shared key still read (vauchi/private#452).
- A waiting delayed sync stops only after the outer relay's delay window
  (`RelayConfig::ohttp_delay_window`, 900 s by default), so its last
  connection does not reveal the forward time (vauchi/private#628).
- The Advanced wipe group reads "Erase data", and the wipe leaves the
  privacy overview (vauchi/private#655, vauchi/private#279).

### Fixed

- A same-second owner-device change arriving in a later sync applies
  (vauchi/private#659).
- Own-card device sync matches entries by id, the apps' visibility toggle
  reaches linked devices, and re-stamps journal nothing
  (vauchi/private#649).
- A device link carries the owner's contacts and own card
  (vauchi/private#658); a linked device syncs before it has contacts, and
  a platform wakeup syncs when due (vauchi/private#648).
- Personal notes saved or deleted in the app reach linked devices
  (vauchi/private#452).
- An emergency alert to a contact not yet heard from is genesis-sealed
  instead of skipped (vauchi/private#633).
- A failed Glance no longer offers to switch to Glance
  (vauchi/private#669); a Glance link opened while Glance shows restarts
  the radio (vauchi/private#672); equal advertised BLE tokens make both
  devices dial (vauchi/private#275).
- After a shred, the reopened engine waits for in-flight calls, drops late
  events for the old screen, and keeps its surface revisions and window
  (vauchi/private#293).
- Device-link request and identity bytes parse through a checked cursor;
  rekey fails on a ciphertext it cannot decrypt (vauchi/private#286).
- Group changes reach the cached Exchange picker (vauchi/private#653);
  the sync chip and backup frequency render in the screen language
  (vauchi/private#423); the screen catalog renders at each shell's own
  size (vauchi/private#420).

## [0.81.0] — 2026-10-12

### Added

- Exchange links and QRs open a landing page for people without the app
  (vauchi/private#277). A link is `https://vauchi.app/x#pk=…&n=…`, with
  the sender's name as `nm=<base64url>` (cut to 64 bytes) when the share
  screen makes it; a QR shows `https://vauchi.app/x#q=<base64url>` through
  `ExchangeQR::to_qr_url` and `OobBootstrapQr::to_qr_url`. The payload stays
  in the URL fragment, which browsers never send to a server.
- Delayed fetch and acknowledgement through the outer relay (#628), off by
  default (`RelayConfig::ohttp_delayed`): a sync posts its fetches under a
  ticket and collects the answers on a later sync. Sends stay
  synchronous. `Vauchi::sync_collecting_delayed` waits within one call for
  the answers it posted, for processes that sync once and exit.

### Changed

- The link and QR parsers accept the https form and the old forms
  (`vauchi://exchange?…`, the raw QR payload, `wb://` on the C ABI), so
  links and codes from older apps still work. A payload under another
  host, path, scheme or parameter is rejected, and a scanned QR over
  4 KiB is refused before decoding. `parse_exchange_deep_link` answers
  `InvalidHost` for an https URL off `vauchi.app/x#`.
- The C ABI's `vauchi_exchange_generate_qr` returns the https URL instead
  of `wb://…`. USB and radio payloads keep the raw form.

### Removed

- Duress alerts (#636, ADR-032 amendment 2026-10-11): a duress unlock
  sends nothing. `AlertKind::Duress`, the `DuressAlertReceived` event, the
  duress notification category and activity-log entry, `DuressSettings`
  with its store and API, and the platform's `ConfigureDuressAlerts` /
  `GetDuressSettings` commands are gone. Duress setup ends at the
  confirmed PIN; migration v81 drops `duress_settings`. A received duress
  alert no longer decodes.

### Fixed

- The exchange picker starts only the modes it offers (private#271); a
  mode removed from the picker cannot be started through its id.
- A delayed sync's send phase no longer collects, and drops, delayed fetch
  answers (#628); pending tickets are bounded, an answer that fails to
  open is reported, and a token registered while a fetch is pending is
  fetched in the same cycle.

## [0.80.2] — 2026-10-11

### Fixed

- A locked session refuses every navigation until it is unlocked
  (vauchi/private#639). Entering the lock screen locks the session; only
  the password, or a biometric unlock without a duress PIN, opens it.
  Until then no navigation or surface switch leaves the lock screen,
  whatever asks for it. An engine built over a Vauchi already opened with
  the password starts unlocked.
- The duress-setup entry flow asks for the app password first when it is
  not given; a duress PIN could be set without knowing it.

## [0.80.1] — 2026-10-11

### Fixed

- Back never leaves the lock screen (vauchi/private#639). Locking pushed
  the screen it covered onto the navigation history, so a Back press
  returned there without the password. The lock screen now offers no
  Back, a Back request there answers as at any root screen, and every
  other Back works as before.

## [0.80.0] — 2026-10-11

### Removed

- `Command::SetAuthenticationRequirement` and `AuthenticationRequirement`
  (vauchi/private#591). Core acts on a biometric unlock itself since
  0.79.0, so shells have nothing to apply; iOS and Android dropped their
  app-password screens. Off the lock screen a biometric unlock now answers
  an empty batch.
- The unsigned OHTTP key bootstrap (vauchi/private#288, plan 7.5). Core
  never requests `GET /v2/ohttp-key`: the key comes only from the signed
  record under the relay's anchor, and a relay without an anchor fails
  closed. `HttpTransport::fetch_ohttp_key` and `OhttpConfig::key_ttl_secs`
  are gone; `probe_ohttp_forward_hop` takes the anchor and the current
  time and verifies the signed record.
- `PurgeRequest`, `PurgeResponse` and their `MessagePayload` variants from
  vauchi-protocol, unused since the HTTP purge (vauchi/private#574).
- The identity-wide self mailbox token; device sync uses the
  recipient-specific token only (vauchi/private#574).

### Changed

- Background syncs follow exponential gaps around 60 s, clamped to
  10 s–5 min, whether or not the app is open (vauchi/private#603).
  `SyncConfig::next_sync_interval` replaces `jittered_sync_interval`, and
  `sync_interval_min_ms`/`sync_interval_max_ms` replace
  `sync_interval_jitter_percent`.
- A vCard nickname becomes the contact's own nickname; `ContactCard` has
  no nickname and its `schema_version` is required (vauchi/private#574).

### Fixed

- The encrypted personal-data export follows the auth mode
  (vauchi/private#616): under the duress PIN it held the real contact
  book. `Vauchi::export_personal_data_encrypted` replaces
  `export_encrypted(storage, …)` for callers holding a `Vauchi`.
- Reads keyed by a real contact's id answer as unknown under the duress
  PIN (vauchi/private#388): shared names and avatars, nickname, custom
  avatar, personal notes, display options and delivery records, and the
  failed-delivery list keeps decoy recipients only.
- `decrypt_with_ad` accepts only ciphertext bound to associated data
  (vauchi/private#574).

## [0.79.2] — 2026-10-11

### Fixed

- The app password form stores the password as typed
  (vauchi/private#621). It echoed one `•` per character and stored the
  value that came back, so a shell typing onto Core's value saved the
  bullets as part of the password. The form now echoes what was typed, as
  the lock screen does; shells mask password inputs (#619).

## [0.79.1] — 2026-10-10

### Fixed

- A duress PIN is stored as typed (vauchi/private#618). The setup read
  every value as digits to append, so a shell sending its whole field on
  each keystroke (Android) stored `665654` for `654321`. A value is now
  the whole field: leading `•` stand for the digits Core already holds,
  digits after them are typed. Non-digits are still dropped and the
  length capped.

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
