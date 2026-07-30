/* SPDX-FileCopyrightText: 2026 Mattia Egloff <mattia.egloff@pm.me> */
/* SPDX-License-Identifier: GPL-3.0-or-later */

#ifndef VAUCHI_CABI_H
#define VAUCHI_CABI_H

#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>

/**
 * Opaque config builder for C ABI consumers.
 *
 * Built via `vauchi_config_new`, configured with `vauchi_config_set_*`
 * functions, consumed by `vauchi_app_create_from_config`, and freed
 * with `vauchi_config_free`.
 */
typedef struct CabiConfig CabiConfig;

/**
 * Opaque handle to an AppEngine instance.
 */
typedef struct VauchiApp VauchiApp;

/**
 * Opaque handle to an exchange session.
 */
typedef struct VauchiExchange VauchiExchange;

/**
 * Type alias for the C event callback function pointer.
 *
 * Called by core when background operations invalidate screen data.
 * `screen_ids_json` is a JSON array of screen ID strings, e.g. `["contacts","sync"]`.
 * `user_data` is the opaque pointer passed to `vauchi_app_set_event_callback`.
 *
 * The string is owned by core and must NOT be freed by the caller.
 */
typedef void (*VauchiEventCallback)(const char *screen_ids_json, void *user_data);

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Create a new config builder with data directory and relay URL.
 *
 * Returns null if `data_dir` is null.
 * If `relay_url` is null, uses the default (`https://relay.vauchi.app`).
 *
 * # Safety
 * `data_dir` and `relay_url` must be valid null-terminated C strings, or null.
 */
struct CabiConfig *vauchi_config_new(const char *data_dir, const char *relay_url);

/**
 * Free a config handle.
 *
 * # Safety
 * `config` must be a pointer returned by `vauchi_config_new`, or null.
 */
void vauchi_config_free(struct CabiConfig *config);

/**
 * Set the storage encryption key (exactly 32 bytes, must not be all-zeros).
 *
 * Returns `false` if key_len != 32, key is all-zeros, config is null, or key is null.
 * Never panics across the FFI boundary.
 *
 * # Safety
 * `config` must be a valid config handle or null.
 * `key` must point to at least `key_len` readable bytes, or be null.
 */
bool vauchi_config_set_storage_key(struct CabiConfig *config,
                                   const uint8_t *key,
                                   uintptr_t key_len);

/**
 * Create an AppEngine from a config builder.
 *
 * The config handle is consumed (freed) by this call — do not free it
 * separately. Returns null on initialization failure or if config is null.
 *
 * # Safety
 * `config` must be a valid config handle returned by `vauchi_config_new`, or null.
 */
struct VauchiApp *vauchi_app_create_from_config(struct CabiConfig *config);

/**
 * Free a string allocated by vauchi-cabi.
 *
 * # Safety
 * `ptr` must be a pointer returned by a vauchi_* function, or null.
 */
void vauchi_string_free(char *ptr);

/**
 * Create a new AppEngine with in-memory storage and default relay.
 *
 * Returns null on initialization failure.
 *
 * # Safety
 * No special requirements.
 */
struct VauchiApp *vauchi_app_create(void);

/**
 * Create a new AppEngine with a custom relay URL.
 *
 * If `relay_url` is null, uses the default (`https://relay.vauchi.app`).
 * The caller retains ownership of the `relay_url` string.
 *
 * Returns null on initialization failure.
 *
 * # Safety
 * `relay_url` must be a valid null-terminated C string, or null.
 */
struct VauchiApp *vauchi_app_create_with_relay(const char *relay_url);

/**
 * Create a new AppEngine with persistent storage and custom relay URL.
 *
 * Unlike `vauchi_app_create` (in-memory), this stores data on disk at
 * `data_dir/vauchi.db`. Pass null for `relay_url` to use the default.
 *
 * Returns null on initialization failure.
 *
 * # Safety
 * `data_dir` must be a valid null-terminated C string pointing to a
 * writable directory. `relay_url` must be a valid null-terminated C
 * string, or null.
 */
struct VauchiApp *vauchi_app_create_with_config(const char *data_dir, const char *relay_url);

/**
 * Create a new AppEngine with persistent storage and caller-provided key.
 *
 * The caller manages key storage (e.g., Windows PasswordVault, platform keychain).
 * `key_bytes` must point to exactly `key_len` bytes. `key_len` must be 32.
 *
 * Returns null on initialization failure or invalid parameters.
 *
 * # Safety
 * `data_dir` must be a valid null-terminated C string pointing to a writable directory.
 * `relay_url` must be a valid null-terminated C string, or null.
 * `key_bytes` must point to at least `key_len` valid bytes, or be null.
 */
struct VauchiApp *vauchi_app_create_with_key(const char *data_dir,
                                             const char *relay_url,
                                             const uint8_t *key_bytes,
                                             uintptr_t key_len);

/**
 * Destroy an AppEngine instance.
 *
 * # Safety
 * `handle` must be a pointer returned by `vauchi_app_create`, or null.
 */
void vauchi_app_destroy(struct VauchiApp *handle);

/**
 * Poll for any new OS notifications produced by the app engine.
 *
 * Returns a JSON-encoded array of `PendingNotification` objects, or
 * null if there are no new notifications.
 *
 * # Safety
 * `app` must be a valid pointer created by `vauchi_app_create*`.
 * The caller must free the returned string via `vauchi_string_free`.
 */
char *vauchi_app_poll_notifications(struct VauchiApp *app);

/**
 * Wake-up tick: run due work and return the next OS notifications plus
 * any commands emitted (in practice the next `Command::ScheduleWakeup`).
 *
 * Returns a JSON-encoded `{"notifications": [...], "commands": [...]}` object,
 * or null on error. The caller must free the returned string via
 * `vauchi_string_free`.
 *
 * # Safety
 * `app` must be a valid pointer created by `vauchi_app_create*`.
 */
char *vauchi_app_on_wakeup(struct VauchiApp *app);

/**
 * Check whether the app has an identity.
 *
 * Returns 1 if an identity exists, 0 if not, -1 on error (null handle, lock failure).
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
int32_t vauchi_app_has_identity(struct VauchiApp *handle);

/**
 * Create a test identity (DEBUG/testing only).
 *
 * Creates an identity with the given display name. No-op if an identity
 * already exists. Returns 0 on success, -1 on error.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `display_name` must be a valid null-terminated C string, or null (defaults to "Test User").
 */
int32_t vauchi_app_create_identity(struct VauchiApp *handle, const char *display_name);

/**
 * Register a callback for async state-change notifications.
 *
 * Core calls `callback` when background operations (sync, delivery,
 * device link) change data that affects rendered screens. Pass null
 * to unregister. `user_data` is forwarded to each callback invocation.
 *
 * # Threading — IMPORTANT
 *
 * The callback may fire **on the same thread** that called
 * `vauchi_app_dispatch` (synchronous event dispatch). The callback
 * **must not** call back into any `vauchi_app_*` function directly —
 * doing so would deadlock on the internal Mutex. Always defer
 * processing to a separate thread or event loop iteration.
 *
 * # Safety
 * `handle` must be a valid `VauchiApp` pointer. `callback` (if non-null)
 * must be safe to call from any thread. `user_data` must remain valid
 * until the callback is unregistered.
 */
void vauchi_app_set_event_callback(struct VauchiApp *handle,
                                   VauchiEventCallback callback,
                                   void *user_data);

/**
 * Create a new AppEngine with persistent storage and platform keyring.
 *
 * Uses `PlatformKeyring` (D-Bus Secret Service on Linux, Keychain on macOS)
 * for secure key storage. Falls back to file-based key storage if the
 * keyring is unavailable.
 *
 * Returns null on initialization failure.
 *
 * # Safety
 * `data_dir` must be a valid null-terminated C string pointing to a
 * writable directory. `relay_url` must be a valid null-terminated C
 * string, or null.
 */
struct VauchiApp *vauchi_app_create_with_keyring(const char *data_dir, const char *relay_url);

/**
 * Import contacts from vCard (.vcf) data.
 *
 * Returns a JSON object on success:
 * ```json
 * {
 *   "imported": 3,
 *   "skipped": 1,
 *   "warnings": [
 *     {"key": "import.warning.duplicate_uid", "args": {"uid": "abc"}, "legacy_text": "Skipped duplicate (UID: abc)"}
 *   ]
 * }
 * ```
 *
 * Each warning object carries the stable i18n `key`, a string map of
 * placeholder `args`, and a pre-rendered English `legacy_text` frontends
 * may use as a fallback. The shape matches the UniFFI
 * `MobileImportWarning` record so CABI + UniFFI consumers stay aligned
 * (G6 of the pure-renderer remediation).
 *
 * Returns `{"error":"..."}` on failure. Returns null if `handle` or
 * `data` is null.
 *
 * The caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `data` must point to at least `data_len` valid bytes, or be null.
 */
char *vauchi_app_import_contacts_from_vcf(struct VauchiApp *handle,
                                          const uint8_t *data,
                                          uintptr_t data_len);

/**
 * Drain pending OS notifications as a JSON array.
 *
 * Returns a JSON array of notification objects, e.g.:
 * `[{"event_key":"...","category":"EmergencyAlert","title":"...","body":"...","contact_id":"..."}]`
 *
 * Returns `"[]"` if no notifications are pending.
 * Returns null on error (null handle, lock poisoned).
 *
 * Frontends should call this after receiving the event callback.
 * Each call clears the buffer — notifications are never returned twice.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_drain_notifications(struct VauchiApp *handle);

/**
 * Return the complete initial Core command batch.
 *
 * # Safety
 * `handle` must be a valid app handle or null. The returned string must be
 * released with `vauchi_string_free`.
 */
char *vauchi_app_initial_commands(struct VauchiApp *handle);

/**
 * Reduce one canonical event into an ordered Core command batch.
 *
 * # Safety
 * `handle` must be a valid app handle or null. `event_json` must be a valid
 * null-terminated C string or null. The returned string must be released with
 * `vauchi_string_free`.
 */
char *vauchi_app_dispatch(struct VauchiApp *handle, const char *event_json);

/**
 * Create a new QR exchange session using the app's identity.
 *
 * Uses manual confirmation for proximity verification (suitable for
 * desktop platforms without audio proximity hardware).
 *
 * Returns null if the app handle is null, identity is not created,
 * or initialization fails.
 *
 * # Safety
 * `app` must be a valid app handle or null.
 */
struct VauchiExchange *vauchi_exchange_create(struct VauchiApp *app);

/**
 * Destroy an exchange session.
 *
 * # Safety
 * `handle` must be a pointer returned by `vauchi_exchange_create`, or null.
 */
void vauchi_exchange_destroy(struct VauchiExchange *handle);

/**
 * Start QR generation and return the QR data string ("wb://...").
 *
 * Returns error JSON if the session is in the wrong state.
 * Caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_generate_qr(struct VauchiExchange *handle);

/**
 * Process a scanned QR code from the peer.
 *
 * `qr_data` should be the full QR string (with or without "wb://" prefix).
 * Returns `"ok"` on success, error JSON on failure.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 * `qr_data` must be a valid null-terminated C string, or null.
 */
char *vauchi_exchange_process_qr(struct VauchiExchange *handle, const char *qr_data);

/**
 * Signal that the peer scanned our QR code.
 *
 * Returns `"ok"` on success, error JSON on failure.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_they_scanned_our_qr(struct VauchiExchange *handle);

/**
 * Perform key agreement and proximity verification.
 *
 * Returns `"ok"` on success, error JSON on failure.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_perform_key_agreement(struct VauchiExchange *handle);

/**
 * Complete the exchange with the peer's card name.
 *
 * Returns `"ok"` on success, error JSON on failure.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 * `their_name` must be a valid null-terminated C string, or null.
 */
char *vauchi_exchange_complete(struct VauchiExchange *handle, const char *their_name);

/**
 * Confirm that the user verified proximity manually.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
void vauchi_exchange_confirm_proximity(struct VauchiExchange *handle);

/**
 * Get the current exchange state as a string label.
 *
 * Returns one of: "idle", "displaying_qr", "peer_scanned",
 * "awaiting_key_agreement", "awaiting_card_exchange", "complete", "failed".
 * Returns null if the handle is null.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_state(struct VauchiExchange *handle);

/**
 * Check whether the exchange session has timed out.
 *
 * Returns 1 if timed out, 0 if not, -1 on error.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
int32_t vauchi_exchange_is_timed_out(struct VauchiExchange *handle);

/**
 * Get the peer's display name (from their QR code).
 *
 * Returns the name string, or null if not yet known or handle is null.
 * Caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_peer_display_name(struct VauchiExchange *handle);

/**
 * Enable debug logging on the exchange session.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
void vauchi_exchange_enable_debug_log(struct VauchiExchange *handle);

/**
 * Get the exchange debug log as JSONL.
 *
 * Returns the JSONL string, or null if debug logging is not enabled.
 * Caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_debug_jsonl(struct VauchiExchange *handle);

/**
 * Get the exchange debug log as Markdown.
 *
 * Returns the Markdown string, or null if debug logging is not enabled.
 * Caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid exchange handle or null.
 */
char *vauchi_exchange_debug_markdown(struct VauchiExchange *handle);

/**
 * Get a translated string for the given locale and key.
 *
 * Returns the translated string, falling back to English if the
 * locale lacks the key, or `"Missing: <key>"` if no translation
 * exists at all. Returns null if `locale_code` or `key` is null,
 * or `locale_code` is not a recognised locale.
 *
 * The caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * `locale_code` and `key` must be valid null-terminated C strings,
 * or null.
 */
char *vauchi_i18n_get_string(const char *locale_code, const char *key);

/**
 * Return a JSON array of all available locale codes.
 *
 * Example: `["en","de","fr","es","it"]`
 *
 * The caller must free the returned string with `vauchi_string_free`.
 *
 * # Safety
 * No special requirements.
 */
char *vauchi_i18n_available_locales(void);

/**
 * Initialise the i18n system from a directory of JSON locale files.
 *
 * Each `*.json` file in `resource_dir` is loaded as a locale
 * (filename stem = locale code, e.g. `de.json` → `"de"`).
 *
 * Returns 0 on success, 1 on failure.
 *
 * # Safety
 * `resource_dir` must be a valid null-terminated C string, or null.
 */
int32_t vauchi_i18n_init(const char *resource_dir);

/**
 * Check whether the i18n system has been initialised.
 *
 * Returns 1 if at least one locale has been loaded, 0 otherwise.
 *
 * # Safety
 * No special requirements.
 */
int32_t vauchi_i18n_is_initialized(void);

/**
 * Install the stderr `tracing` subscriber. Safe to call more than once —
 * only the first call takes effect.
 *
 * # Safety
 * No special requirements.
 */
void vauchi_cabi_init_logging(void);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* VAUCHI_CABI_H */
