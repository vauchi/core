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
 * Opaque handle to a workflow engine instance.
 */
typedef struct VauchiWorkflow VauchiWorkflow;

/**
 * Create a new config builder with data directory and relay URL.
 *
 * Returns null if `data_dir` is null.
 * If `relay_url` is null, uses the default (`wss://relay.vauchi.app`).
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
 * If `relay_url` is null, uses the default (`wss://relay.vauchi.app`).
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
 * "device_linking", "duress_pin", "delivery_status", "sync",
 * "tor_settings", "recovery", "groups", "privacy", "support",
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

#endif  /* VAUCHI_CABI_H */
