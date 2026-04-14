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
 * Opaque handle to a device link initiator.
 */
typedef struct VauchiDeviceLinkInitiator VauchiDeviceLinkInitiator;

/**
 * Opaque handle to an exchange session.
 */
typedef struct VauchiExchange VauchiExchange;

/**
 * Opaque handle to a workflow engine instance.
 */
typedef struct VauchiWorkflow VauchiWorkflow;

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
 * Enable or disable BLE backend.
 *
 * # Safety
 * `config` must be a valid config handle or null.
 */
void vauchi_config_enable_ble(struct CabiConfig *config, bool enabled);

/**
 * Enable or disable audio (ultrasonic) backend.
 *
 * # Safety
 * `config` must be a valid config handle or null.
 */
void vauchi_config_enable_audio(struct CabiConfig *config, bool enabled);

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
 * Get the current screen as a JSON string.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_current_screen(struct VauchiApp *handle);

/**
 * Poll for any new OS notifications produced by the app engine.
 *
 * Returns a JSON-encoded array of `PendingNotification` objects, or
 * null if there are no new notifications.
 *
 * # Safety
 * `app` must be a valid pointer created by `vauchi_app_create*`.
 * The caller must free the returned string via `vauchi_free_string`.
 */
char *vauchi_app_poll_notifications(struct VauchiApp *app);

/**
 * Handle a user action (JSON) and return the result as JSON.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `action_json` must be a valid null-terminated C string, or null.
 */
char *vauchi_app_handle_action(struct VauchiApp *handle, const char *action_json);

/**
 * Navigate to a screen by name. Returns the new screen as JSON.
 *
 * Supported screen names: "home", "contacts", "exchange", "settings",
 * "help", "backup", "lock", "onboarding", "emergency_shred",
 * "device_linking", "device_management", "duress_pin", "delivery_status",
 * "sync", "recovery", "groups", "privacy", "support",
 * "contact_duplicates", "contact_limit", "more".
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `screen_name` must be a valid null-terminated C string, or null.
 */
char *vauchi_app_navigate_to(struct VauchiApp *handle, const char *screen_name);

/**
 * Get available screens as a JSON array of strings.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_available_screens(struct VauchiApp *handle);

/**
 * Returns the default landing screen as a C string ("my_info" or "contacts").
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_default_screen(struct VauchiApp *handle);

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
 * Handle a hardware event during an exchange (ADR-031).
 *
 * `event_json` must be a JSON-encoded `ExchangeHardwareEvent`.
 * Returns the action result as JSON, or null if the event was ignored
 * (e.g., not on the exchange screen).
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `event_json` must be a valid null-terminated C string, or null.
 */
char *vauchi_app_handle_hardware_event(struct VauchiApp *handle, const char *event_json);

/**
 * Notify the engine that the app moved to the background.
 *
 * If a password is set and the app is not already locked or in
 * onboarding, navigates to the lock screen and returns the lock
 * screen JSON. Otherwise returns null.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_handle_app_backgrounded(struct VauchiApp *handle);

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
 * `vauchi_app_handle_action` (synchronous event dispatch). The callback
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
 * Signal that a peer device has connected during device linking.
 *
 * `verification_code` is the code to display for manual confirmation.
 * Returns the updated screen JSON, or null if not on the device linking
 * screen.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `verification_code` must be a valid null-terminated C string, or null.
 */
char *vauchi_app_device_link_peer_connected(struct VauchiApp *handle,
                                            const char *verification_code);

/**
 * Signal that data sync has completed during device linking.
 *
 * Returns the updated screen JSON, or null if not on the device linking
 * screen.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_app_device_link_sync_complete(struct VauchiApp *handle);

/**
 * Import contacts from vCard (.vcf) data.
 *
 * Returns a JSON object: `{"imported":N,"skipped":N,"warnings":["..."]}`,
 * or `{"error":"..."}` on failure. Returns null if `handle` or `data` is null.
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
 * Check if audio proximity verification is available on this platform.
 *
 * Returns 1 if cpal can enumerate at least one output and one input device,
 * 0 otherwise. Always safe to call (never panics).
 *
 * # Safety
 * No special requirements.
 */
int32_t vauchi_audio_is_available(void);

/**
 * Emit an ultrasonic challenge signal containing `data`.
 *
 * Blocks until the signal has been emitted. Returns 1 on success, 0 on failure.
 * `data` must point to at least `data_len` valid bytes.
 *
 * # Safety
 * `data` must be a valid pointer to `data_len` bytes, or null.
 */
int32_t vauchi_audio_emit(const uint8_t *data, uintptr_t data_len);

/**
 * Listen for an ultrasonic response within `timeout_ms` milliseconds.
 *
 * Blocks until a response is received or the timeout expires.
 * Returns a JSON string `{"data":[1,2,3,...]}` on success, or null on
 * failure/timeout. The caller must free the returned string with
 * `vauchi_string_free`.
 *
 * # Safety
 * No special requirements.
 */
char *vauchi_audio_listen(uint64_t timeout_ms);

/**
 * Stop all audio operations. Cancels any in-flight emit or listen.
 *
 * # Safety
 * No special requirements.
 */
void vauchi_audio_stop(void);

/**
 * Start a device link as the existing device (initiator).
 *
 * Creates an initiator from the app's identity and device registry.
 * Returns null if no identity exists or on error.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
struct VauchiDeviceLinkInitiator *vauchi_device_link_start(struct VauchiApp *handle);

/**
 * Destroy a device link initiator.
 *
 * # Safety
 * `initiator` must be a pointer returned by `vauchi_device_link_start`, or null.
 */
void vauchi_device_link_initiator_destroy(struct VauchiDeviceLinkInitiator *initiator);

/**
 * Get the QR data string from the initiator.
 *
 * # Safety
 * `initiator` must be a valid initiator handle or null.
 */
char *vauchi_device_link_qr_data(struct VauchiDeviceLinkInitiator *initiator);

/**
 * Get the expiry timestamp (Unix seconds) of the QR code.
 *
 * Returns 0 on error.
 *
 * # Safety
 * `initiator` must be a valid initiator handle or null.
 */
uint64_t vauchi_device_link_expires_at(struct VauchiDeviceLinkInitiator *initiator);

/**
 * Decrypt an incoming link request and return confirmation details.
 *
 * `encrypted_request_b64` is the base64-encoded encrypted request from
 * the new device. Returns a JSON string:
 * `{"device_name":"...","confirmation_code":"...","identity_fingerprint":"..."}`
 * or `{"error":"..."}` on failure. Returns null on null inputs.
 *
 * # Safety
 * `initiator` must be a valid initiator handle or null.
 * `encrypted_request_b64` must be a valid null-terminated C string, or null.
 */
char *vauchi_device_link_prepare_confirmation(struct VauchiDeviceLinkInitiator *initiator,
                                              const char *encrypted_request_b64);

/**
 * Confirm the device link with manual code verification.
 *
 * Must call `vauchi_device_link_prepare_confirmation` first.
 * `confirmation_code` is the human-readable code (e.g. "123-456").
 * Rust computes the HMAC internally — the link key never crosses FFI.
 * `confirmed_at` is the Unix timestamp (seconds).
 *
 * Returns JSON: `{"encrypted_response":"base64...","device_name":"...","device_index":N}`
 * or `{"error":"..."}`. Returns null on null inputs.
 *
 * # Safety
 * `initiator` must be a valid initiator handle or null.
 * `confirmation_code` must be a valid null-terminated C string, or null.
 */
char *vauchi_device_link_confirm_manual(struct VauchiDeviceLinkInitiator *initiator,
                                        const char *confirmation_code,
                                        uint64_t confirmed_at);

/**
 * Listen for an incoming device link request via relay (blocking).
 *
 * Creates an exchange offer with the identity, then polls until the new
 * device claims it. Blocks up to `timeout_secs` seconds.
 *
 * Returns JSON: `{"encrypted_payload":"base64...","sender_token":"..."}`
 * or `{"error":"..."}`. Returns null on null handle.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 */
char *vauchi_device_link_listen(struct VauchiApp *handle, uint64_t timeout_secs);

/**
 * Send device link response back via relay.
 *
 * Claims the return channel created by the new device.
 * Returns 0 on success, -1 on error.
 *
 * # Safety
 * `handle` must be a valid app handle or null.
 * `sender_token` and `encrypted_response_b64` must be valid null-terminated
 * C strings, or null.
 */
int32_t vauchi_device_link_send_response(struct VauchiApp *handle,
                                         const char *sender_token,
                                         const char *encrypted_response_b64);

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
 * Create a new workflow engine instance.
 *
 * Supported `workflow_type` values:
 * - `"onboarding"` — onboarding flow (no args)
 * - `"emergency_shred"` — emergency data wipe (no args)
 * - `"lock_screen"` — lock screen with 3 max attempts (no args)
 *
 * Returns null on unknown type or null input.
 *
 * # Safety
 * `workflow_type` must be a valid null-terminated C string, or null.
 */
struct VauchiWorkflow *vauchi_workflow_create(const char *workflow_type);

/**
 * Destroy a workflow engine instance.
 *
 * # Safety
 * `handle` must be a pointer returned by `vauchi_workflow_create`, or null.
 */
void vauchi_workflow_destroy(struct VauchiWorkflow *handle);

/**
 * Get the current screen as a JSON string.
 *
 * Returns null if the handle is null. Returns an error JSON object if
 * the internal lock is poisoned. The caller must free the returned
 * string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid workflow handle or null.
 */
char *vauchi_workflow_current_screen(struct VauchiWorkflow *handle);

/**
 * Handle a user action (JSON string) and return the result as JSON.
 *
 * Returns null if the handle is null. Returns an error JSON object if
 * the action JSON is null or invalid. The caller must free the returned
 * string with `vauchi_string_free`.
 *
 * # Safety
 * `handle` must be a valid workflow handle or null.
 * `action_json` must be a valid null-terminated C string, or null.
 */
char *vauchi_workflow_handle_action(struct VauchiWorkflow *handle, const char *action_json);

#ifdef __cplusplus
}  // extern "C"
#endif  // __cplusplus

#endif  /* VAUCHI_CABI_H */
